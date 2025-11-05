use color_eyre::Result;

use db::init_db;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let project_dirs = ProjectDirs::from("com", "tukanoid", "leaper")
        .ok_or_eyre("Failed to get project directories");
    let db = init_db(project_dirs).await?;

    Ok(())
}
