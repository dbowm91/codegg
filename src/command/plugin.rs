use clap::Subcommand;

use crate::plugin::marketplace::{MarketplacePlugin, MarketplaceService, PluginTier};

#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// List installed plugins
    List,
    /// Search available plugins
    Search {
        /// Search query
        query: String,
    },
    /// Install a plugin
    Install {
        /// Plugin source (path, URL, or name)
        source: String,
    },
}

pub async fn run_plugin_command(cmd: PluginCommand) -> anyhow::Result<()> {
    let marketplace = MarketplaceService::new();

    match cmd {
        PluginCommand::List => {
            let plugins = marketplace.list_local_plugins().await;
            if plugins.is_empty() {
                println!("No plugins installed.");
            } else {
                println!("Installed plugins:\n");
                for plugin in plugins {
                    println_plugin(&plugin);
                }
            }
        }
        PluginCommand::Search { query } => {
            let results = marketplace.search_plugins(&query).await;
            if results.is_empty() {
                println!("No plugins found matching '{query}'");
            } else {
                println!("Search results for '{query}':\n");
                for plugin in results {
                    println_plugin(&plugin);
                }
            }
        }
        PluginCommand::Install { source } => {
            println!("Installing plugin from: {source}");
            if source.starts_with("http://") || source.starts_with("https://") {
                crate::plugin::install::install_from_url(&source).await?;
            } else {
                let path = std::path::Path::new(&source);
                crate::plugin::install::install_from_path(path).await?;
            }
            println!("Plugin installed successfully.");
        }
    }

    Ok(())
}

fn println_plugin(plugin: &MarketplacePlugin) {
    println!(
        "[{}] {} v{}",
        plugin.tier,
        plugin.name,
        plugin.version
    );
    if let Some(desc) = &plugin.description {
        println!("  {}", desc);
    }
    if let Some(author) = &plugin.author {
        println!("  Author: {}", author);
    }
    if !plugin.hooks.is_empty() {
        println!("  Hooks: {}", plugin.hooks.join(", "));
    }
    println!();
}
