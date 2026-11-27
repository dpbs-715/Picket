pub mod git;

#[cfg(feature = "smoke-harness")]
use crate::pi_runtime::{PiEvent, PiPaths, PiSupervisor, ProbeReport};
#[cfg(feature = "smoke-harness")]
use serde::Serialize;

#[cfg(feature = "smoke-harness")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundationReport {
    pub complete: ProbeReport,
    pub cancelled: ProbeReport,
    pub shutdown: ProbeReport,
    pub git: git::GitProbeReport,
}

#[cfg(feature = "smoke-harness")]
pub async fn smoke(paths: &PiPaths) -> Result<FoundationReport, String> {
    let supervisor = PiSupervisor::default();
    let complete = supervisor
        .run_probe(paths, "smoke-complete".into(), |_| Ok(()))
        .await?;
    let cancelled = supervisor
        .run_probe(paths, "smoke-cancel".into(), |event| {
            if matches!(event, PiEvent::TextDelta { .. }) {
                supervisor.cancel("smoke-cancel")?;
            }
            Ok(())
        })
        .await?;
    let other = supervisor.clone();
    let shutdown_paths = paths.clone();
    let (started, ready) = tokio::sync::oneshot::channel();
    let mut started = Some(started);
    let pending = tokio::spawn(async move {
        other
            .run_probe(&shutdown_paths, "smoke-shutdown".into(), |event| {
                if matches!(event, PiEvent::Started { .. })
                    && let Some(started) = started.take()
                {
                    let _ = started.send(());
                }
                Ok(())
            })
            .await
    });
    ready.await.map_err(|e| e.to_string())?;
    supervisor.shutdown().await?;
    let shutdown = pending.await.map_err(|e| e.to_string())??;
    if complete.status != "completed"
        || cancelled.status != "cancelled"
        || shutdown.status != "cancelled"
    {
        return Err("Unexpected Pi lifecycle result".into());
    }
    let git = tokio::task::spawn_blocking(git::run_probe)
        .await
        .map_err(|e| e.to_string())??;
    Ok(FoundationReport {
        complete,
        cancelled,
        shutdown,
        git,
    })
}
