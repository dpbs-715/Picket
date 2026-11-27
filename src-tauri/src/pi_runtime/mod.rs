mod process;
mod protocol;
mod supervisor;
#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "smoke-harness"))]
pub use protocol::PiEvent;
pub use protocol::{
    CustomActionRunMode, PiCommand, PiConflictFileSummary, PiPlanFileEntry, PiProviderSpec,
    PiTaskOutcome, PiToolExecutionResult, RequestContext,
};
#[cfg(feature = "smoke-harness")]
pub use supervisor::ProbeReport;
pub use supervisor::{PiPaths, PiSupervisor};
