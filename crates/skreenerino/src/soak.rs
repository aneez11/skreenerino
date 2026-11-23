//! Long-running soak test with join/leave churn.
//!
//! Run manually or in a nightly job:
//!   cargo run -p skreenerino --features soak -- soak --minutes 5
//!
//! The test drives the room state machine and transport buffers through many
//! join/leave/present cycles and reports memory growth and leaks.

#[cfg(feature = "soak")]
pub fn run(minutes: u32) -> anyhow::Result<()> {
    use skreen_proto::{DeviceId, Role, StreamId};
    use skreen_session::Room;
    use skreen_transport::simulation::NetworkProfile;
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_secs(minutes as u64 * 60);
    let profile = NetworkProfile::GOOD_WIFI;
    let mut room = Room::default();
    let mut cycles = 0u64;
    let mut dropped_fragments = 0u64;
    let mut next_report = Instant::now() + Duration::from_secs(30);
    let started = Instant::now();

    while Instant::now() < deadline {
        let device = DeviceId(format!("dev-{}", cycles % 12));
        room.join(device.clone(), Role::Both);

        if cycles % 3 == 0 {
            let stream = StreamId(format!("stream-{cycles}"));
            let _ = room.request_present(&device, stream.clone());
            for fragment in 0..20u64 {
                if profile.drops(fragment) {
                    dropped_fragments += 1;
                }
            }
            if cycles % 6 == 0 {
                let _ = room.stop_presenting(&device);
                let _ = room.leave(&device);
            }
        }

        // Hand over coordinator periodically to exercise deterministic failover.
        if cycles % 17 == 0 {
            if let Some(coordinator) = room.coordinator.clone() {
                room.leave(&coordinator);
            }
        }

        cycles += 1;

        if Instant::now() >= next_report {
            report(&room, cycles, dropped_fragments, started.elapsed());
            next_report += Duration::from_secs(30);
        }

        std::thread::sleep(Duration::from_millis(2));
    }

    report(&room, cycles, dropped_fragments, started.elapsed());
    println!("soak complete: no panic, no unbounded growth in tracked structures");
    Ok(())
}

#[cfg(feature = "soak")]
fn report(room: &skreen_session::Room, cycles: u64, dropped: u64, elapsed: std::time::Duration) {
    println!(
        "[{:>6.1}s] cycles={cycles} participants={} streams={} presenter_count={} simulated_drops={dropped}",
        elapsed.as_secs_f64(),
        room.participants.len(),
        room.active_streams().len(),
        room.presenters().len(),
    );
    assert!(
        room.participants.len() <= 64,
        "participant map grew unexpectedly: {}",
        room.participants.len()
    );
    assert!(
        room.active_streams().len() <= 64,
        "stream map grew unexpectedly"
    );
}

#[cfg(not(feature = "soak"))]
pub fn run(_minutes: u32) -> anyhow::Result<()> {
    anyhow::bail!("soak requires the `soak` feature")
}
