//! Why one pull request lookup failed. It keeps the typed cause; only the
//! place that shows it turns it into text.

use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub(crate) enum LookupError {
    /// The key names an account this window holds no token for.
    #[error("not signed in to GitHub")]
    NotSignedIn,
    /// GitHub reported a lifecycle this client does not know.
    #[error("unknown pull request state")]
    UnknownState,
    /// GitHub, the network or the response failed. One failed request can
    /// leave several keys unanswered, so they share the cause.
    #[error("GitHub lookup failed")]
    Github {
        #[source]
        source: Arc<crate::Error>,
    },
}
