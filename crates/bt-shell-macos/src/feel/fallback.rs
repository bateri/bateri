//! AppKit's own haptic feedback, for a system whose framework cannot be driven: the three fixed
//! patterns it has, which the system plays only while a finger is on the glass.
//!
//! What is lost is told here and not hidden: a held event is not played (AppKit has no such thing)
//! and every tap is the same generic click, whatever its strength and sharpness. AppKit is the main
//! thread's: the click is handed to the main queue, from the thread that times the pattern.

use dispatch2::DispatchQueue;
use objc2_app_kit::{
    NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
    NSHapticFeedbackPerformer,
};

use super::player::Driver;
use super::steps::Step;

pub(super) struct Fallback;

impl Driver for Fallback {
    fn begin(&mut self) -> Option<usize> {
        // AppKit knows where the finger is and plays nothing where there is none.
        Some(0)
    }

    fn fire(&mut self, _target: usize, step: &Step) -> bool {
        if step.tap {
            DispatchQueue::main().exec_async(|| {
                NSHapticFeedbackManager::defaultPerformer().performFeedbackPattern_performanceTime(
                    NSHapticFeedbackPattern::Generic,
                    NSHapticFeedbackPerformanceTime::Now,
                );
            });
        }
        true
    }
}
