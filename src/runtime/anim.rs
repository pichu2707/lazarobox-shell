//! A tick armed only while something needs it: the thinking indicator's
//! animation while a chat stream runs, the memory wait while a chat awaits a
//! reply. One interval each, gone the rest of the time, so an idle app wakes
//! for nothing.

use std::time::Duration;

use tokio::time::{Instant, Interval, MissedTickBehavior, interval_at};

/// Time between animation frames.
pub const PERIOD: Duration = Duration::from_millis(100);

/// Time between the ticks that give up a lost memory reply. The wait itself
/// is `MEMORY_WAIT`; a tick only has to notice it ran out.
pub const MEMORY_PERIOD: Duration = Duration::from_millis(500);

/// One armed-on-demand interval of the event loop.
pub struct Animation {
    interval: Option<Interval>,
    period: Duration,
}

impl Default for Animation {
    fn default() -> Self {
        Self::every(PERIOD)
    }
}

impl Animation {
    pub fn new() -> Self {
        Self::default()
    }

    /// A tick every `period`.
    pub fn every(period: Duration) -> Self {
        Self {
            interval: None,
            period,
        }
    }

    /// Arms the interval when a stream runs and none is armed; drops it when
    /// none runs. Arming again while armed keeps the running phase.
    pub fn sync(&mut self, running: bool) {
        if !running {
            self.interval = None;
        } else if self.interval.is_none() {
            let mut interval = interval_at(Instant::now() + self.period, self.period);
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
            self.interval = Some(interval);
        }
    }

    pub fn is_armed(&self) -> bool {
        self.interval.is_some()
    }

    /// Resolves at the next frame; never resolves while disarmed. A late
    /// tick is skipped, not replayed: after a long stall one tick is ready
    /// and the next waits for its slot.
    pub async fn tick(&mut self) {
        match self.interval.as_mut() {
            Some(interval) => {
                interval.tick().await;
            }
            None => std::future::pending().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;
    use tokio::time::advance;

    use super::*;
    use crate::{
        app::{App, AppEvent},
        core::layout::PaneId,
    };

    /// Ticks ready right now.
    fn ready(anim: &mut Animation) -> usize {
        let mut n = 0;
        while anim.tick().now_or_never().is_some() {
            n += 1;
            if n > 1000 {
                break;
            }
        }
        n
    }

    /// Runs `ms` virtual milliseconds in 1 ms steps, counting the ticks.
    async fn run(anim: &mut Animation, ms: u64) -> usize {
        let mut ticks = 0;
        for _ in 0..ms {
            advance(Duration::from_millis(1)).await;
            ticks += ready(anim);
        }
        ticks
    }

    #[tokio::test(start_paused = true)]
    async fn nothing_is_armed_until_a_stream_runs() {
        let mut anim = Animation::new();
        assert!(!anim.is_armed());
        assert_eq!(run(&mut anim, 10_000).await, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_running_stream_ticks_ten_times_a_second() {
        let mut anim = Animation::new();
        anim.sync(true);
        assert!(anim.is_armed());
        let ticks = run(&mut anim, 1000).await;
        assert_eq!(ticks, 10);
        assert!(ticks <= 12);
    }

    #[tokio::test(start_paused = true)]
    async fn the_tick_goes_away_with_the_last_stream() {
        let mut anim = Animation::new();
        anim.sync(true);
        assert_eq!(run(&mut anim, 300).await, 3);
        anim.sync(false);
        assert!(!anim.is_armed());
        assert_eq!(run(&mut anim, 10_000).await, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn arming_again_while_armed_keeps_the_phase() {
        let mut anim = Animation::new();
        anim.sync(true);
        run(&mut anim, 90).await;
        anim.sync(true);
        assert_eq!(
            run(&mut anim, 10).await,
            1,
            "the slot at 100 ms still comes"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn missed_ticks_are_skipped_not_replayed() {
        let mut anim = Animation::new();
        anim.sync(true);
        advance(Duration::from_secs(5)).await;
        assert_eq!(ready(&mut anim), 1, "one pending tick after 5 s");
        assert_eq!(ready(&mut anim), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn it_restarts_after_an_idle_period_without_a_burst() {
        let mut anim = Animation::new();
        anim.sync(true);
        run(&mut anim, 250).await;
        anim.sync(false);
        advance(Duration::from_secs(30)).await;
        anim.sync(true);
        assert_eq!(ready(&mut anim), 0, "no catch-up");
        assert_eq!(run(&mut anim, 99).await, 0);
        assert_eq!(run(&mut anim, 1).await, 1);
    }

    /// An app with `n` chats, each sending a message.
    fn streaming_app(n: u32) -> App {
        let mut app = App::new(200, 25);
        for i in 0..n {
            app.open_chat(None);
            let id = app.focused();
            assert_ne!(id, PaneId::FIRST, "chat {i}");
            app.chat_send(id, "hi");
        }
        app
    }

    #[tokio::test(start_paused = true)]
    async fn three_streams_share_one_tick() {
        let app = streaming_app(3);
        assert_eq!(app.streams().len(), 3);
        let mut anim = Animation::new();
        anim.sync(app.animating());
        assert_eq!(run(&mut anim, 1000).await, 10);
    }

    #[tokio::test(start_paused = true)]
    async fn idle_chat_panes_never_tick() {
        let mut app = App::new(200, 25);
        app.open_chat(None);
        let mut anim = Animation::new();
        anim.sync(app.animating());
        assert!(!anim.is_armed());
        assert_eq!(run(&mut anim, 10_000).await, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn the_tick_follows_the_app_through_every_way_a_stream_ends() {
        let mut anim = Animation::new();
        // A cancel, a closed pane and a quit-less end all disarm it.
        let mut app = streaming_app(1);
        anim.sync(app.animating());
        assert!(anim.is_armed());
        let id = app.focused();
        app.chat_cancel(id);
        anim.sync(app.animating());
        assert!(!anim.is_armed(), "cancel");

        let mut app = streaming_app(1);
        anim.sync(app.animating());
        assert!(anim.is_armed());
        for c in [' ', 'w', 'q', 'y'] {
            let key = if c == ' ' {
                ratatui::crossterm::event::KeyEvent::new(
                    ratatui::crossterm::event::KeyCode::Char(' '),
                    ratatui::crossterm::event::KeyModifiers::CONTROL,
                )
            } else {
                ratatui::crossterm::event::KeyEvent::new(
                    ratatui::crossterm::event::KeyCode::Char(c),
                    ratatui::crossterm::event::KeyModifiers::NONE,
                )
            };
            app.update(AppEvent::Key(key));
        }
        anim.sync(app.animating());
        assert!(!anim.is_armed(), "pane close");
    }
}

#[cfg(test)]
mod memory_tests {
    use tokio::time::advance;

    use super::*;
    use crate::{
        app::{App, AppEvent},
        core::layout::PaneId,
    };

    /// The virtual clock of tokio as the app's monotonic clock.
    fn virtual_now() -> std::time::Instant {
        Instant::now().into_std()
    }

    /// Runs the loop's memory timer for `ms` virtual milliseconds in steps of
    /// 100 ms, feeding every tick to `app` and re-syncing the timer after each
    /// step, as `run_loop` does.
    async fn run(app: &mut App, watch: &mut Animation, ms: u64) {
        for _ in 0..ms / 100 {
            advance(Duration::from_millis(100)).await;
            watch.sync(app.memory_waiting());
            while futures::FutureExt::now_or_never(watch.tick()).is_some() {
                app.update(AppEvent::MemoryTick);
                watch.sync(app.memory_waiting());
            }
        }
    }

    fn lines(app: &App) -> Vec<String> {
        use crate::app::PaneView;
        let Some(PaneView::Chat(chat)) = app.pane_view(app.focused()) else {
            panic!("not a chat");
        };
        chat.entries()
            .iter()
            .filter_map(|e| match e {
                crate::core::ai::chat::Entry::System(s) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    /// An app whose chat asked for its project and is never answered.
    fn waiting_app() -> App {
        use crate::core::ai::config::{Assignment, MemoryConfig, Task};
        let mut ai = crate::core::ai::config::AiConfig::default();
        ai.set(
            Task::Chat,
            Assignment {
                engine: Some("ollama".into()),
                model: Some("llama3:8b".into()),
            },
        );
        ai.memory = MemoryConfig {
            enabled: true,
            acknowledged: true,
            ..MemoryConfig::default()
        };
        let mut app = App::new(80, 25).with_ai(ai).with_clock(virtual_now);
        app.update(AppEvent::Cwd(PaneId::FIRST, "/srv/app".into()));
        let key = |c| {
            AppEvent::Key(ratatui::crossterm::event::KeyEvent::new(
                ratatui::crossterm::event::KeyCode::Char(c),
                ratatui::crossterm::event::KeyModifiers::NONE,
            ))
        };
        app.update(AppEvent::Key(ratatui::crossterm::event::KeyEvent::new(
            ratatui::crossterm::event::KeyCode::Char(' '),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        )));
        app.update(key('c'));
        app
    }

    #[tokio::test(start_paused = true)]
    async fn nothing_is_armed_while_no_chat_waits_for_memory() {
        let app = App::new(80, 25);
        let mut watch = Animation::every(MEMORY_PERIOD);
        watch.sync(app.memory_waiting());
        assert!(!watch.is_armed());
    }

    #[tokio::test(start_paused = true)]
    async fn a_lost_reply_is_given_up_after_five_virtual_seconds_and_the_timer_goes() {
        let mut app = waiting_app();
        let mut watch = Animation::every(MEMORY_PERIOD);
        watch.sync(app.memory_waiting());
        assert!(watch.is_armed(), "a chat waits for its project");
        run(&mut app, &mut watch, 4_800).await;
        assert!(lines(&app).is_empty(), "not before the bound");
        assert!(watch.is_armed());
        run(&mut app, &mut watch, 1_200).await;
        assert_eq!(
            lines(&app),
            ["memory unavailable: engram did not answer"],
            "given up, once"
        );
        assert!(!watch.is_armed(), "nothing waits: the timer is gone");
    }
}
