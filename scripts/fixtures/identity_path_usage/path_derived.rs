// Intentional negative fixture for check_identity_path_usage.py --fixture.
fn invalid_authority(path: &str) {
    let _ = codegg_core::identity::ProjectId::parse(path);
    let directory = path;
    let project_dir = path;
    let _ = (directory, project_dir);
    let _ = state.project_dir;
}
