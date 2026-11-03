//! Where the user has been. Only what the user does adds to it or moves in it: a page has no way
//! to touch it, and whatever sends a page on by itself must replace the entry and not add one, so
//! that Back always goes to where the user last chose to be.

/// How many places are remembered.
const MAX_ENTRIES: usize = 1000;

/// A place the user has been.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    /// The address, without a fragment.
    pub address: String,
    pub fragment: String,
    pub scroll_y: i32,
}

#[derive(Clone, Debug, Default)]
pub struct History {
    entries: Vec<Entry>,
    current: usize,
}

impl History {
    /// Goes to a new place, which forgets those that Back had left. Going where the user already
    /// is adds nothing.
    pub fn visit(&mut self, address: &str, fragment: &str) {
        if let Some(entry) = self.current()
            && entry.address == address
            && entry.fragment == fragment
        {
            return;
        }
        self.entries.truncate(self.current + 1);
        if self.entries.len() == MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push(Entry {
            address: address.to_owned(),
            fragment: fragment.to_owned(),
            scroll_y: 0,
        });
        self.current = self.entries.len() - 1;
    }

    /// Remembers how far down the current place was, for coming back to it.
    pub fn save_scroll(&mut self, scroll_y: i32) {
        if let Some(entry) = self.entries.get_mut(self.current) {
            entry.scroll_y = scroll_y;
        }
    }

    #[must_use]
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.current)
    }

    #[must_use]
    pub fn can_go_back(&self) -> bool {
        self.current > 0
    }

    #[must_use]
    pub fn can_go_forward(&self) -> bool {
        self.current + 1 < self.entries.len()
    }

    /// Returns whether there was somewhere to go.
    pub fn go_back(&mut self) -> bool {
        let can = self.can_go_back();
        if can {
            self.current -= 1;
        }
        can
    }

    /// Returns whether there was somewhere to go.
    pub fn go_forward(&mut self) -> bool {
        let can = self.can_go_forward();
        if can {
            self.current += 1;
        }
        can
    }
}
