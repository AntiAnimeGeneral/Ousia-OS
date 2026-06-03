#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IpcPayload {
    words: [u64; MAX_IPC_WORDS],
    len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IpcError {
    TooManyMessageWords { requested: usize, limit: usize },
}

pub const MAX_IPC_WORDS: usize = 4;

impl IpcPayload {
    pub const fn empty() -> Self {
        Self {
            words: [0; MAX_IPC_WORDS],
            len: 0,
        }
    }

    pub fn new(words: &[u64]) -> Result<Self, IpcError> {
        if words.len() > MAX_IPC_WORDS {
            return Err(IpcError::TooManyMessageWords {
                requested: words.len(),
                limit: MAX_IPC_WORDS,
            });
        }

        let mut payload = Self::empty();
        payload.words[..words.len()].copy_from_slice(words);
        payload.len = words.len();
        Ok(payload)
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn truncate_to_words(self, words: usize) -> Self {
        let len = if words < self.len { words } else { self.len };
        let mut payload = Self::empty();
        payload.words[..len].copy_from_slice(&self.words[..len]);
        payload.len = len;
        payload
    }

    pub fn words(&self) -> &[u64] {
        &self.words[..self.len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_rejects_too_many_words() {
        // Goal: IPC payload construction enforces the fixed message-register limit.
        // Scope: unit test for payload normalization before any endpoint or scheduler side effect.
        // Semantics: oversized input fails at the boundary and does not create a partial payload.

        assert_eq!(
            IpcPayload::new(&[1, 2, 3, 4, 5]),
            Err(IpcError::TooManyMessageWords {
                requested: 5,
                limit: MAX_IPC_WORDS,
            })
        );
    }

    #[test]
    fn payload_truncate_keeps_message_word_prefix() {
        // Goal: message length consumption never exposes words past the requested prefix.
        // Scope: unit test for IPC payload normalization before endpoint delivery.
        // Semantics: truncating is monotonic and never pads missing message registers.
        let payload = IpcPayload::new(&[1, 2, 3]).unwrap();

        assert_eq!(payload.truncate_to_words(2).words(), &[1, 2]);
        assert_eq!(payload.truncate_to_words(8).words(), &[1, 2, 3]);
        assert_eq!(payload.truncate_to_words(0), IpcPayload::empty());
    }
}
