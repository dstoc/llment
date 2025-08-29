use std::sync::{Arc, Mutex};

use tokio::sync::{OnceCell, mpsc::UnboundedSender, oneshot};

use llm::LlmClient;

use crate::{
    app::Update,
    components::completion::{Command, CommandInstance, Completion, CompletionResult},
};

pub struct ModelCommand {
    pub(crate) client: Arc<Mutex<llm::Client>>,
    pub(crate) tx: UnboundedSender<Update>,
}

impl Command for ModelCommand {
    fn name(&self) -> &'static str {
        "model"
    }
    fn description(&self) -> &'static str {
        "Change the active model"
    }
    fn has_params(&self) -> bool {
        true
    }
    fn instance(&self) -> Box<dyn CommandInstance> {
        Box::new(ModelCommandInstance {
            tx: self.tx.clone(),
            client: self.client.clone(),
            models: Arc::default(),
            param: String::default(),
        })
    }
}

struct ModelCommandInstance {
    tx: UnboundedSender<Update>,
    client: Arc<Mutex<llm::Client>>,
    models: Arc<OnceCell<Vec<String>>>,
    param: String,
}

impl ModelCommandInstance {
    fn matching(&self) -> Vec<Completion> {
        if let Some(models) = self.models.get() {
            let param = self.param.as_str();
            models
                .iter()
                .filter(|model| model.starts_with(param))
                .map(|model| Completion {
                    name: model.clone(),
                    description: "".to_string(),
                    str: model.clone(),
                })
                .collect()
        } else {
            Vec::new()
        }
    }
}

impl CommandInstance for ModelCommandInstance {
    fn update(&mut self, input: &str) -> CompletionResult {
        let param = input.trim();
        self.param = param.to_string();
        if self.models.get().is_some() {
            let options = self.matching();
            CompletionResult::Options { at: 0, options }
        } else {
            let client_handle = self.client.clone();
            let models = self.models.clone();
            let (tx, rx) = oneshot::channel();
            tokio::spawn(async move {
                let client = { client_handle.lock().unwrap().clone() };
                let _ = models
                    .get_or_init(|| async move {
                        match client.list_models().await {
                            Ok(models) => models,
                            Err(_) => Vec::new(), // TODO: surface an error?
                        }
                    })
                    .await;

                let _ = tx.send(());
            });
            CompletionResult::Loading { at: 0, done: rx }
        }
    }
    fn commit(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.param.is_empty() {
            Err("no param".into())
        } else {
            println!("commit model??");
            let _ = self.tx.send(Update::SetModel(self.param.clone()));
            Ok(())
        }
    }
}
