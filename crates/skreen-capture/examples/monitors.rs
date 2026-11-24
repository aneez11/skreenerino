fn main() -> anyhow::Result<()> {
    for m in skreen_capture::list_monitors()? {
        println!(
            "id={} name={} {}x{} at {},{} primary={}",
            m.id,
            m.label(),
            m.width,
            m.height,
            m.x,
            m.y,
            m.is_primary
        );
    }
    let (bytes, w, h) = skreen_capture::monitor_thumbnail(0, 320)?;
    println!("thumbnail 0: {w}x{h}, {} bytes", bytes.len());
    Ok(())
}
