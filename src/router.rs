use crate::call::{CallCustom, CallResponse};
use crate::handler::{Handler, HandlerContext, IntoHandlerResponse, Unsigned};
use crate::impl_handler::IntoHandler;
use std::collections::HashMap;
use tracing::error;

pub struct Router {
  handlers: HashMap<String, Box<dyn Handler>>,
}

impl Router {
  pub fn new() -> Self {
    Self {
      handlers: HashMap::new(),
    }
  }

  pub fn handle<F, Args>(mut self, name: &str, handler: F) -> Self
  where
    F: IntoHandler<Args>,
  {
    if self.handlers.contains_key(name) {
      todo!("duplicate handler name: {}", name);
    }
    self.handlers.insert(name.to_string(), handler.into_handler());
    self
  }

  pub async fn dispatch(&self, name: &str, ctx: HandlerContext) -> Box<dyn IntoHandlerResponse> {
    let Some(handler) = self.handlers.get(name) else {
      // An empty success: a panic here drops the connection and the game retries until it
      // shows "Connection Error"
      error!(name, "no handler for this method, answering with an empty success");
      let response: CallResponse<dyn CallCustom> = CallResponse::new_success(Box::new(serde_json::json!({})));
      return Box::new(Unsigned(response));
    };

    handler.call(ctx).await
  }
}
