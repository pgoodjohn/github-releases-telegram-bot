use async_trait::async_trait;
use std::error::Error;
use teloxide::prelude::*;

use crate::ports::Messenger;

pub struct TeloxideMessenger {
    bot: Bot,
}

impl TeloxideMessenger {
    pub fn new(bot: Bot) -> Self {
        Self { bot }
    }
}

#[async_trait]
impl Messenger for TeloxideMessenger {
    async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<teloxide::types::ParseMode>,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        let mut request = self.bot.send_message(ChatId(chat_id), text);
        if let Some(mode) = parse_mode {
            request = request.parse_mode(mode);
        }
        request.await?;
        Ok(())
    }
}
