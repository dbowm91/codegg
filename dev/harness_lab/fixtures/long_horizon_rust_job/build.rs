fn main() {
    // A deterministic short compile window lets the scripted native ACP lane
    // perform independent reads while the scheduler-owned test job is active.
    std::thread::sleep(std::time::Duration::from_secs(2));
}
