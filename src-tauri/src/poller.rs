//! The background polling loop. A single long-lived tokio task: it ticks on an
//! interval (or an on-demand refresh signal), fetches every provider
//! concurrently under a timeout, folds results into state, fires transition
//! notifications, and pushes the new snapshot to the tray + frontend.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::config;
use crate::providers::Provider;
use crate::state::AppState;

const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

pub async fn run(app: AppHandle) {
    loop {
        poll_once(&app).await;

        let interval = config::effective_interval(&app);
        let state = app.state::<AppState>();
        tokio::select! {
            _ = tokio::time::sleep(interval) => {}
            _ = state.refresh.notified() => {}
        }
    }
}

pub async fn poll_once(app: &AppHandle) {
    let state = app.state::<AppState>();

    // Clone the provider Arcs out and drop the guard before any await.
    let providers: Vec<Arc<dyn Provider>> = state.registry.read().unwrap().clone();

    if !providers.is_empty() {
        let fetches = providers.into_iter().map(|p| async move {
            let res = tokio::time::timeout(FETCH_TIMEOUT, p.fetch_monitors()).await;
            (p, res)
        });
        let results = futures::future::join_all(fetches).await;

        let mut transitions = Vec::new();
        for (p, res) in results {
            match res {
                Ok(Ok(monitors)) => {
                    transitions.extend(state.apply_success(
                        p.id(),
                        p.display_name(),
                        p.kind(),
                        monitors,
                    ));
                }
                Ok(Err(crate::providers::ProviderError::RateLimited)) => {
                    // Transient backpressure, not a check failure. Leave the last
                    // good state intact (do NOT advance the failure counter, so a
                    // 429 never escalates monitors to Unknown).
                    log::warn!("provider {} rate-limited; keeping last state", p.display_name());
                }
                Ok(Err(e)) => {
                    log::warn!("provider {} fetch failed: {e}", p.display_name());
                    state.apply_failure(p.id());
                }
                Err(_) => {
                    log::warn!("provider {} timed out", p.display_name());
                    state.apply_failure(p.id());
                }
            }
        }

        for t in &transitions {
            crate::notify::fire(app, t);
        }
    }

    // Stamp the completion so the popover-open gate knows how fresh status is.
    state.mark_polled();
    push_update(app, &state);
}

/// Push the current snapshot to the tray icon and the frontend.
fn push_update(app: &AppHandle, state: &AppState) {
    let agg = state.aggregate();
    log::info!(
        "poll complete: {} monitors ({} up, {} down, {} unknown)",
        agg.total(),
        agg.up,
        agg.down,
        agg.unknown
    );
    crate::tray::apply_aggregate(app, agg);
    let _ = app.emit("monitors:updated", state.snapshot_view());
}
