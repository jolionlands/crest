//! ModuleRuntime drives the lifecycle of each instantiated module:
//! - Constructs instances from the registry using the config's ModuleEntry list.
//! - Calls Module::initial() once at startup.
//! - Schedules periodic Module::tick() calls per the module's interval().
//! - Provides a snapshot of all current ModuleSnapshots to the renderer.

use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;

use crate::config::types::{ModuleEntry, ModulesConfig};
use super::{BarRegion, Module, ModuleEvent, ModuleRegistry, ModuleSnapshot};

// ---------------------------------------------------------------------------
// ModuleInstance — one live module slot
// ---------------------------------------------------------------------------

struct ModuleInstance {
    module: Box<dyn Module>,
    region: BarRegion,
    next_tick: Instant,
    /// Index into the shared snapshots Vec.
    snapshot_idx: usize,
    /// Forwarded from the config entry for on-event dispatch.
    on_click: Option<String>,
    on_scroll_up: Option<String>,
    on_scroll_down: Option<String>,
}

// ---------------------------------------------------------------------------
// ModuleRuntime
// ---------------------------------------------------------------------------

pub struct ModuleRuntime {
    instances: Vec<ModuleInstance>,
    snapshots: Arc<RwLock<Vec<ModuleSnapshot>>>,
}

impl ModuleRuntime {
    /// Build runtime from config + registry.
    ///
    /// Iterates `config.left + center + right`, looks up each `kind` in the
    /// registry, calls its factory, and calls `Module::initial()` to populate
    /// the snapshot buffer.  Unknown kinds are silently skipped with a warning.
    pub fn new(config: &ModulesConfig, registry: &ModuleRegistry) -> Self {
        let mut instances: Vec<ModuleInstance> = Vec::new();
        let mut initial_snaps: Vec<ModuleSnapshot> = Vec::new();

        let zones: [(&[ModuleEntry], BarRegion); 3] = [
            (&config.left,   BarRegion::Left),
            (&config.center, BarRegion::Center),
            (&config.right,  BarRegion::Right),
        ];

        for (entries, region) in &zones {
            for entry in *entries {
                match registry.build(entry) {
                    Some(mut module) => {
                        let mut snap = module.initial();
                        // Stamp the region from config (modules may not set it correctly).
                        snap.region = *region;

                        let idx = initial_snaps.len();
                        initial_snaps.push(snap);

                        instances.push(ModuleInstance {
                            next_tick: Instant::now() + module.interval(),
                            region: *region,
                            snapshot_idx: idx,
                            on_click: entry.on_click.clone(),
                            on_scroll_up: entry.on_scroll_up.clone(),
                            on_scroll_down: entry.on_scroll_down.clone(),
                            module,
                        });
                    }
                    None => {
                        tracing::warn!(
                            kind = %entry.kind,
                            "unknown module kind — skipping"
                        );
                    }
                }
            }
        }

        Self {
            instances,
            snapshots: Arc::new(RwLock::new(initial_snaps)),
        }
    }

    /// Returns a clone of the current snapshots for the renderer.
    pub fn snapshots(&self) -> Vec<ModuleSnapshot> {
        self.snapshots.read().clone()
    }

    /// Returns an `Arc` pointing at the shared snapshot buffer so callers can
    /// hold a cheap reference without cloning every tick.
    pub fn snapshots_ref(&self) -> Arc<RwLock<Vec<ModuleSnapshot>>> {
        Arc::clone(&self.snapshots)
    }

    /// Tick all modules whose schedule has elapsed.
    ///
    /// Returns `true` if any snapshot text changed — the caller should
    /// invalidate the bar window in that case.
    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let mut changed = false;

        for instance in self.instances.iter_mut() {
            if now >= instance.next_tick {
                let mut new_snap = instance.module.tick();
                // Stamp the correct region so the renderer zones work.
                new_snap.region = instance.region;

                instance.next_tick = now + instance.module.interval();

                let mut snaps = self.snapshots.write();
                let old = &snaps[instance.snapshot_idx];
                if old.text != new_snap.text || old.fg != new_snap.fg {
                    changed = true;
                }
                snaps[instance.snapshot_idx] = new_snap;
            }
        }

        changed
    }

    /// Dispatch a click/scroll event to the module under the cursor x.
    ///
    /// Zone assignment is coarse (left third / center third / right third).
    ///
    /// TODO(audit): replace with precise per-module hit testing once the
    /// renderer exposes per-module pixel rects.
    ///
    /// Returns an optional shell command to spawn (config `on_click` /
    /// `on_scroll_*`), or the string returned by `Module::on_event`.
    pub fn dispatch_event(
        &mut self,
        x: i32,
        bar_w: u32,
        event: ModuleEvent,
    ) -> Option<String> {
        let zone = coarse_zone(x, bar_w);

        // Find the first module in that zone and call on_event.
        for instance in self.instances.iter_mut() {
            if instance.region != zone {
                continue;
            }
            // Config-level command takes priority over the module's own handler.
            let config_cmd = match event {
                ModuleEvent::LeftClick   => instance.on_click.clone(),
                ModuleEvent::ScrollUp    => instance.on_scroll_up.clone(),
                ModuleEvent::ScrollDown  => instance.on_scroll_down.clone(),
                _ => None,
            };
            if config_cmd.is_some() {
                return config_cmd;
            }
            return instance.module.on_event(event);
        }
        None
    }
}

/// Map an x coordinate to a `BarRegion` using simple thirds.
fn coarse_zone(x: i32, bar_w: u32) -> BarRegion {
    let w = bar_w as i32;
    if x < w / 3 {
        BarRegion::Left
    } else if x < 2 * w / 3 {
        BarRegion::Center
    } else {
        BarRegion::Right
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use crate::config::types::{ModuleEntry, ModulesConfig};
    use crate::module::{BarRegion, Module, ModuleEvent, ModuleRegistry, ModuleSnapshot};

    // -----------------------------------------------------------------------
    // EchoModule — returns a fixed string, interval 1 s
    // -----------------------------------------------------------------------
    struct EchoModule {
        text: String,
    }

    impl Module for EchoModule {
        fn kind(&self) -> &'static str { "echo" }
        fn initial(&self) -> ModuleSnapshot {
            ModuleSnapshot {
                text: self.text.clone(),
                fg: None,
                icon: None,
                region: BarRegion::Left,
            }
        }
        fn tick(&mut self) -> ModuleSnapshot { self.initial() }
        fn interval(&self) -> Duration { Duration::from_secs(1) }
    }

    // -----------------------------------------------------------------------
    // SlowModule — 10 s interval
    // -----------------------------------------------------------------------
    struct SlowModule {
        ticked: bool,
    }

    impl Module for SlowModule {
        fn kind(&self) -> &'static str { "slow" }
        fn initial(&self) -> ModuleSnapshot {
            ModuleSnapshot { text: "init".to_string(), ..Default::default() }
        }
        fn tick(&mut self) -> ModuleSnapshot {
            self.ticked = true;
            ModuleSnapshot { text: "ticked".to_string(), ..Default::default() }
        }
        fn interval(&self) -> Duration { Duration::from_secs(10) }
    }

    fn make_entry(kind: &str) -> ModuleEntry {
        ModuleEntry { kind: kind.to_string(), ..Default::default() }
    }

    fn make_registry_with_echo() -> ModuleRegistry {
        let mut reg = ModuleRegistry::new();
        reg.register(
            "echo",
            Box::new(|_entry| Box::new(EchoModule { text: "hello".to_string() })),
        );
        reg
    }

    // -----------------------------------------------------------------------
    // Test 1: initial() populates snapshots
    // -----------------------------------------------------------------------
    #[test]
    fn test_module_runtime_initial_populates_snapshots() {
        let registry = make_registry_with_echo();
        let config = ModulesConfig {
            left: vec![make_entry("echo")],
            center: vec![],
            right: vec![],
        };
        let runtime = ModuleRuntime::new(&config, &registry);
        let snaps = runtime.snapshots();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].text, "hello");
        assert_eq!(snaps[0].region, BarRegion::Left);
    }

    // -----------------------------------------------------------------------
    // Test 2: tick respects the module's interval
    // -----------------------------------------------------------------------
    #[test]
    fn test_module_runtime_tick_respects_interval() {
        let mut reg = ModuleRegistry::new();
        // Use a cell to let the factory produce a module we can observe later.
        // Simplest approach: just check snapshot text changes.
        reg.register(
            "slow",
            Box::new(|_entry| Box::new(SlowModule { ticked: false })),
        );
        let config = ModulesConfig {
            left: vec![make_entry("slow")],
            center: vec![],
            right: vec![],
        };
        let mut runtime = ModuleRuntime::new(&config, &reg);

        // Just after construction next_tick is ~10 s in the future → tick returns false.
        let changed = runtime.tick();
        assert!(!changed, "tick should not fire within the 10 s interval");

        // Manually push next_tick into the past so the module fires next call.
        runtime.instances[0].next_tick =
            Instant::now() - Duration::from_millis(1);
        let changed = runtime.tick();
        assert!(changed, "tick should fire once next_tick has elapsed");
        assert_eq!(runtime.snapshots()[0].text, "ticked");
    }

    // -----------------------------------------------------------------------
    // Test 3: dispatch_event routes to a Left-zone module
    // -----------------------------------------------------------------------
    #[test]
    fn test_dispatch_event_left_zone() {
        struct ClickModule;
        impl Module for ClickModule {
            fn kind(&self) -> &'static str { "clicker" }
            fn initial(&self) -> ModuleSnapshot { ModuleSnapshot::default() }
            fn tick(&mut self) -> ModuleSnapshot { ModuleSnapshot::default() }
            fn on_event(&mut self, event: ModuleEvent) -> Option<String> {
                if event == ModuleEvent::LeftClick {
                    Some("echo clicked".to_string())
                } else {
                    None
                }
            }
        }

        let mut reg = ModuleRegistry::new();
        reg.register("clicker", Box::new(|_| Box::new(ClickModule)));

        let config = ModulesConfig {
            left: vec![make_entry("clicker")],
            center: vec![],
            right: vec![make_entry("clicker")],
        };
        let mut runtime = ModuleRuntime::new(&config, &reg);
        let bar_w = 1200u32;

        // x = 0 is in the left zone → first left module → returns "echo clicked"
        let result = runtime.dispatch_event(0, bar_w, ModuleEvent::LeftClick);
        assert_eq!(result.as_deref(), Some("echo clicked"));

        // x in right zone → right module
        let result_right = runtime.dispatch_event(1100, bar_w, ModuleEvent::LeftClick);
        assert_eq!(result_right.as_deref(), Some("echo clicked"));
    }
}
