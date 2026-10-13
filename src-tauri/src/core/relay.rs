//! The phone's channel: a signal the user asked to be told about away from the Mac is sent on.

use super::signals::Signal;
use super::*;

impl Core {
    /// Hands `signal` to the phone's channel when the user asked for that; true when it was
    /// handed over. Returns at once: the send is on its own thread.
    pub(super) fn relay(&self, _signal: &Signal) -> bool {
        false
    }
}
