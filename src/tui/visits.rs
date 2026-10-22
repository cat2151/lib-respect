use std::collections::BTreeSet;

#[derive(Default)]
pub(super) struct Visits {
    opened: BTreeSet<String>,
    pending: BTreeSet<String>,
    in_flight: BTreeSet<String>,
    ready: bool,
}

impl Visits {
    pub fn opened(&mut self, url: String) {
        self.opened.insert(url.clone());
        self.pending.insert(url);
    }

    pub fn returned(&mut self) {
        // A later return may follow an earlier key press before the browser opened.
        self.pending.append(&mut self.opened);
        self.ready = true;
    }

    pub fn key_pressed(&mut self) {
        // Fallback for terminals which do not report focus changes.
        self.ready = !self.pending.is_empty();
    }

    pub fn due(&self) -> BTreeSet<String> {
        if self.ready {
            self.pending.clone()
        } else {
            BTreeSet::new()
        }
    }

    pub fn scheduled(&mut self) -> BTreeSet<String> {
        self.in_flight.extend(self.due());
        self.pending.retain(|url| !self.in_flight.contains(url));
        self.ready = false;
        self.in_flight.clone()
    }

    pub fn completed(&mut self, url: &str) {
        self.in_flight.remove(url);
    }

    pub fn finished(&mut self) {
        self.in_flight.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_after_return_not_before_browser_interaction_without_polling() {
        let mut visits = Visits::default();
        let url = "https://github.com/example/lib".to_owned();
        visits.opened(url.clone());
        assert!(visits.due().is_empty());
        visits.key_pressed();
        assert_eq!(visits.due(), BTreeSet::from([url.clone()]));
        visits.scheduled();
        // Replacing a running background job must retain its forced targets.
        assert_eq!(visits.scheduled(), BTreeSet::from([url.clone()]));
        visits.completed(&url);
        assert!(visits.scheduled().is_empty());
        visits.key_pressed();
        assert!(visits.due().is_empty());
        visits.returned();
        assert_eq!(visits.due(), BTreeSet::from([url.clone()]));
        visits.scheduled();
        visits.completed(&url);
        visits.returned();
        assert!(visits.due().is_empty());
        visits.opened(url.clone());
        visits.returned();
        assert_eq!(visits.due(), BTreeSet::from([url]));
    }
}
