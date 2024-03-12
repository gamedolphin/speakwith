use std::{path::Path, sync::Arc};

use anyhow::Result;
use minijinja::{Environment, ErrorKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::RwLock;
use rust_embed::RustEmbed;

use crate::FrontendError;

#[derive(RustEmbed)]
#[folder = "./templates"]
struct TemplateFiles;

#[derive(Clone)]
pub struct Templates {
    env: Arc<RwLock<Environment<'static>>>,
    _watcher: Arc<RecommendedWatcher>,
}

fn split(val: String) -> Result<Vec<String>, minijinja::Error> {
    let out = val.split("||").map(|v| v.into()).collect::<Vec<String>>();

    Ok(out)
}

impl Default for Templates {
    fn default() -> Self {
        let mut env = Environment::new();
        minijinja_contrib::add_to_environment(&mut env);
        env.set_loader(embedded_loader);
        env.add_filter("split", split);

        let env = Arc::new(RwLock::new(env));

        let watched = env.clone();

        let mut watcher = notify::recommended_watcher(move |res| match res {
            Ok(_) => {
                watched.write().clear_templates();
                println!("cleared templates");
            }
            Err(e) => println!("watch error: {:?}", e),
        })
        .unwrap();

        if cfg!(debug_assertions) {
            // Add a path to be watched. All files and directories at that path and
            // below will be monitored for changes.
            watcher
                .watch(
                    Path::new("./pkg/frontend/templates"),
                    RecursiveMode::Recursive,
                )
                .unwrap();
        }

        Self {
            env,
            _watcher: Arc::new(watcher),
        }
    }
}

impl Templates {
    pub fn render_template<S: serde::Serialize>(
        &self,
        name: &str,
        ctx: S,
    ) -> Result<String, FrontendError> {
        self.env
            .read()
            .get_template(name)
            .map_err(|e| {
                println!("get template failed: {:?}", e);
                FrontendError::NotFound(name.to_string())
            })?
            .render(ctx)
            .map_err(|e| {
                println!("render template failed: {:?}", e);
                FrontendError::InternalError(e.into())
            })
    }
}

fn embedded_loader(name: &str) -> Result<Option<String>, minijinja::Error> {
    let Some(file) = TemplateFiles::get(name) else {
        return Ok(None);
    };

    let val = String::from_utf8(file.data.to_vec())
        .map_err(|_| minijinja::Error::from(ErrorKind::CannotDeserialize))?;

    Ok(Some(val))
}
