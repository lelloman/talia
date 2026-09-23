use super::*;
impl Store {
    pub fn telegram_ingest(&mut self, update: &Value, now: i64) -> Result<()> {
        self.alert_atomic(|s|{
            let mut c=s.telegram_config()?;let uid=update["update_id"].as_i64().filter(|v|*v>=0&&*v<i64::MAX).ok_or("invalid update ID")?;
            if uid<c.offset{return Ok(());}c.offset=uid+1;s.telegram_put(&c)?;
            let m=if update["message"].is_object(){&update["message"]}else{&update["channel_post"]};
            let (Some(chat),Some(text))=(m["chat"]["id"].as_i64(),m["text"].as_str())else{return Ok(());};
            if text.len()>16384{return Ok(());}
            let parts:Vec<_>=text.split_whitespace().collect();
            let command=parts.first().map(|v|v.split('@').next().unwrap_or(v)).unwrap_or("");
            if ["/start","/pair"].contains(&command)&&parts.len()==2 {
                let code=parts[1];
                let body=json!({"chat":chat,"kind":m["chat"]["type"],"name":m["chat"]["title"].as_str().or(m["chat"]["first_name"].as_str()).unwrap_or("Telegram chat"),"user":if m["sender_chat"].is_null()&&m["from"]["is_bot"]==false{m["from"]["id"].clone()}else{Value::Null},"user_name":m["from"]["username"].as_str().or(m["from"]["first_name"].as_str()).unwrap_or("")});
                s.conn.execute("UPDATE telegram_pairs SET body=? WHERE code=? AND expires>? AND body IS NULL",params![body.to_string(),code,now]).map_err(err)?;return Ok(());
            }
            let Some(user)=m["from"]["id"].as_i64().filter(|v|*v>0)else{return Ok(());};
            if !m["sender_chat"].is_null()||m["from"]["is_bot"]!=false||!s.telegram_authorized(chat,user)? {return Ok(());}
            let reply=m["reply_to_message"]["message_id"].as_i64();
            let reference:Option<(String,Option<String>)>=if let Some(id)=reply{s.conn.query_row("SELECT kind,reference FROM telegram_outbox WHERE chat=? AND message_id=? AND status='sent'",params![chat,id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(err)?}else{None};
            if m["chat"]["type"]!="private"&&command!="/ask"&&reference.is_none()&&!matches!(command,"/new"|"/compact"){return Ok(());}
            s.telegram_enqueue(&format!("disabled-{uid}"),chat,"chat",Some(&user.to_string()),"Investigations are currently unavailable. Report and alert delivery still work.")?;
            Ok(())
        })
    }
}
