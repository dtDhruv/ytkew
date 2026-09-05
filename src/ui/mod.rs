pub mod banner;
pub mod bigtext;
pub mod views;
pub mod widgets;

/// Which pane is focused. Tab cycles in this order, matching kew's F2..F6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Queue,
    Library,
    Track,
    Search,
    Help,
    Lyrics,
}

impl View {
    /// kew's Tab order skips Help and Lyrics -- they are destinations, not
    /// stops on the cycle.
    pub fn next(self) -> Self {
        match self {
            View::Queue => View::Library,
            View::Library => View::Track,
            View::Track => View::Search,
            View::Search => View::Queue,
            View::Help | View::Lyrics => View::Track,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            View::Queue => View::Search,
            View::Library => View::Queue,
            View::Track => View::Library,
            View::Search => View::Track,
            View::Help | View::Lyrics => View::Track,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            View::Queue => "queue",
            View::Library => "library",
            View::Track => "track",
            View::Search => "search",
            View::Help => "help",
            View::Lyrics => "lyrics",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_cycles_the_four_main_views() {
        let mut v = View::Queue;
        let mut seen = vec![v];
        for _ in 0..3 {
            v = v.next();
            seen.push(v);
        }
        assert_eq!(
            seen,
            vec![View::Queue, View::Library, View::Track, View::Search]
        );
        assert_eq!(v.next(), View::Queue, "should wrap");
    }

    #[test]
    fn prev_is_the_inverse_of_next() {
        for v in [View::Queue, View::Library, View::Track, View::Search] {
            assert_eq!(v.next().prev(), v);
        }
    }

    #[test]
    fn help_and_lyrics_return_to_track() {
        assert_eq!(View::Help.next(), View::Track);
        assert_eq!(View::Lyrics.prev(), View::Track);
    }
}
