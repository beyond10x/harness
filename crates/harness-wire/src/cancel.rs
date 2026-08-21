use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// One cancellation token, shared by everything working on a turn.
///
/// It is deliberately one token rather than one per layer. A person who asks a run to stop expects
/// the model read, the loop and the tool sequence to stop together; separate flags mean the answer
/// arrives anyway because the layer that was actually blocked never heard.
///
/// Cloning shares the state, so a token handed to a signal handler or a reader thread cancels the
/// work the main thread is doing.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clone_cancels_the_original() {
        let token = Cancel::new();
        let elsewhere = token.clone();
        assert!(!token.is_cancelled());
        elsewhere.cancel();
        assert!(
            token.is_cancelled(),
            "a token that does not share state cancels nothing"
        );
    }

    #[test]
    fn a_token_stays_cancelled() {
        // Deliberately one-way. A resettable token invites clearing one the reading thread has
        // just set, which silently revives work its owner stopped; fresh work takes a fresh token.
        let token = Cancel::new();
        token.cancel();
        assert!(token.is_cancelled());
        assert!(token.clone().is_cancelled());
    }
}
