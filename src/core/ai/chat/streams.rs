//! Which running stream the statusline indicator shows. Pure.

use std::time::Instant;

use super::StreamPhase;
use crate::core::layout::PaneId;

/// One running stream, as the statusline needs it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StreamInfo {
    pub pane: PaneId,
    pub started: Instant,
    pub phase: StreamPhase,
    /// The model of the request as sent; empty when the engine chose it.
    pub model: String,
}

/// The stream to show and how many other streams run: the focused pane's
/// stream if it is running, else the most recently started one (ties go to
/// the higher pane id, which is the newer pane).
pub fn pick_stream(streams: &[StreamInfo], focused: PaneId) -> Option<(&StreamInfo, usize)> {
    let shown = streams
        .iter()
        .find(|s| s.pane == focused)
        .or_else(|| streams.iter().max_by_key(|s| (s.started, s.pane)))?;
    Some((shown, streams.len() - 1))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn id(n: u32) -> PaneId {
        PaneId::for_test(n)
    }

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn info(pane: u32, started: Instant, model: &str) -> StreamInfo {
        StreamInfo {
            pane: id(pane),
            started,
            phase: StreamPhase::Thinking,
            model: model.to_owned(),
        }
    }

    #[test]
    fn nothing_running_shows_nothing() {
        assert_eq!(pick_stream(&[], id(1)), None);
    }

    #[test]
    fn a_single_stream_has_no_others() {
        let base = Instant::now();
        let streams = [info(2, base, "m1")];
        let (shown, others) = pick_stream(&streams, id(1)).unwrap();
        assert_eq!((shown.model.as_str(), others), ("m1", 0));
    }

    #[test]
    fn the_focused_streaming_pane_wins_over_a_newer_stream() {
        let base = Instant::now();
        let streams = [info(2, base, "m1"), info(3, at(base, 500), "m2")];
        let (shown, others) = pick_stream(&streams, id(2)).unwrap();
        assert_eq!((shown.model.as_str(), others), ("m1", 1));
    }

    #[test]
    fn without_a_focused_stream_the_most_recent_start_wins() {
        let base = Instant::now();
        let streams = [info(2, base, "m1"), info(3, at(base, 500), "m2")];
        for focused in [1, 4] {
            let (shown, others) = pick_stream(&streams, id(focused)).unwrap();
            assert_eq!((shown.model.as_str(), others), ("m2", 1), "{focused}");
        }
        // The order of the slice does not matter.
        let reversed = [streams[1].clone(), streams[0].clone()];
        assert_eq!(pick_stream(&reversed, id(1)).unwrap().0.model, "m2");
    }

    #[test]
    fn three_streams_count_two_others() {
        let base = Instant::now();
        let streams = [
            info(2, base, "a"),
            info(3, at(base, 1), "b"),
            info(4, at(base, 2), "c"),
        ];
        let (shown, others) = pick_stream(&streams, id(1)).unwrap();
        assert_eq!((shown.model.as_str(), others), ("c", 2));
    }

    #[test]
    fn equal_start_instants_pick_the_newer_pane() {
        let base = Instant::now();
        let streams = [info(5, base, "new"), info(3, base, "old")];
        assert_eq!(pick_stream(&streams, id(1)).unwrap().0.model, "new");
    }

    #[test]
    fn a_stream_that_ended_drops_out_at_once() {
        let base = Instant::now();
        let mut streams = vec![info(2, base, "m1"), info(3, at(base, 500), "m2")];
        streams.retain(|s| s.pane != id(3));
        let (shown, others) = pick_stream(&streams, id(1)).unwrap();
        assert_eq!((shown.model.as_str(), others), ("m1", 0));
        streams.clear();
        assert_eq!(pick_stream(&streams, id(1)), None);
    }
}
