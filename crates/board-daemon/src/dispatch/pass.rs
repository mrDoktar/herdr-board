use std::collections::HashSet;
use std::sync::Arc;

use board_core::model::{Card, Run, SpaceKey};
use tracing::Instrument;

use crate::dispatch::launch_plan::spawn_one;
use crate::state::Daemon;

/// Evaluate the queue and promote as many queued runs as the per-space FIFO and
/// the global concurrency cap allow.
///
/// One span per pass, so the launches it fans out are attributable to the pass
/// that decided them. Passes are serialized, so this is not a hot loop.
#[tracing::instrument(name = "dispatch_pass", skip_all)]
pub(crate) async fn dispatch_pass(d: &Arc<Daemon>) {
    // A claim lives in this pass until spawn registration/failure is durable.
    // Serializing passes prevents another caller from observing those claimed
    // rows as queued and independently claiming the same capacity or space.
    let _pass = d.dispatch_pass.lock().await;
    let active = match d.store.active_runs() {
        Ok(v) => v,
        Err(_) => {
            tracing::warn!(error_category = "database", "dispatch: active_runs failed");
            return;
        }
    };
    // Spaces with a live run block their queue only when runs are serial per
    // space. Either way, at most one launch per space starts in a pass, so two
    // launches never race on the same workspace.
    let mut busy: HashSet<SpaceKey> = if d.config.serial_per_space {
        active
            .iter()
            .map(|(_, card)| SpaceKey::from_card(card))
            .collect()
    } else {
        HashSet::new()
    };
    let mut active_count = active.len();
    let max = d.config.max_concurrent.max(1);

    let queued = match d.store.queued_runs() {
        Ok(v) => v,
        Err(_) => {
            tracing::warn!(error_category = "database", "dispatch: queued_runs failed");
            return;
        }
    };

    // Claim capacity and one FIFO head per space before any launch starts.
    // Independent spaces then launch concurrently; a second run for a claimed
    // space cannot slip in while its first launch is in flight.
    let mut claimed = Vec::new();
    let mut deferred = false;
    for (run, card) in queued {
        if active_count >= max {
            break;
        }
        let key = SpaceKey::from_card(&card);
        if busy.insert(key) {
            active_count += 1;
            claimed.push((run, card));
        } else if !d.config.serial_per_space {
            deferred = true;
        }
    }

    if claimed.is_empty() {
        return;
    }
    tracing::debug!(claimed = claimed.len(), active_count, max, "dispatch pass");

    let mut launches = tokio::task::JoinSet::new();
    for (run, card) in claimed {
        let daemon = Arc::clone(d);
        // Carry this pass's span into the spawned task; a bare `spawn` would
        // otherwise root each launch span on its own.
        let pass_span = tracing::Span::current();
        launches.spawn(
            async move {
                let run_id = run.id;
                (run_id, spawn_one(&daemon, &run, &card).await)
            }
            .instrument(pass_span),
        );
    }
    while let Some(result) = launches.join_next().await {
        match result {
            Ok((_, Ok(true) | Ok(false))) => {}
            Ok((run_id, Err(_))) => {
                tracing::error!(
                    run_id,
                    error_category = "launch",
                    "dispatch: spawn_one failed"
                );
            }
            Err(_) => tracing::error!(error_category = "task", "dispatch: launch task failed"),
        }
    }
    // Launches skipped only because their space already launched this pass go
    // in the next pass, which sees this pass's runs as registered.
    if deferred {
        d.wake_dispatch();
    }
}

/// Select placement for dispatch. v11 rows use the enqueue-time run snapshot;
/// pre-v11 rows explicitly retain the historical current-card behavior.
pub(crate) fn launch_session<'a>(run: &'a Run, card: &'a Card) -> Option<&'a str> {
    if run.launch_spec.is_some() {
        run.session.as_deref()
    } else {
        card.session.as_deref()
    }
}
