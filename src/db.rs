use rusqlite::Connection;

use crate::consts;

pub struct Db {
    connection: Connection,
}

impl Db {
    pub fn new_with_file(filename: &str) -> anyhow::Result<Self> {
        let connection = Connection::open(filename)?;
        Ok(Self { connection })
    }

    pub fn get_messages_id(&self, chat_id: i64, count: u32) -> anyhow::Result<Vec<i32>> {
        let statement = format!("SELECT message_id FROM g{chat_id} ORDER BY id DESC LIMIT ?",);

        let mut statement = self.connection.prepare(&statement)?;
        let mut rows = statement.query([count])?;

        let mut message_ids = Vec::new();
        while let Some(row) = rows.next()? {
            message_ids.push(row.get(0)?);
        }

        Ok(message_ids)
    }

    pub fn add_message_id(&self, chat_id: i64, message_id: i32) -> anyhow::Result<()> {
        // First we have to check if we have a table with the chat_id name. If not we have to create it.
        // Then we have to insert the message_id into the table.
        // Also, we need maintain the table size to be consts::MESSAGE_TO_STORE messages.

        let table_statement = format!(
            "CREATE TABLE IF NOT EXISTS g{chat_id} (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                message_id INTEGER NOT NULL
            )",
        );

        self.connection.execute(&table_statement, [])?;

        let insert_statement =
            format!("INSERT INTO g{chat_id} (timestamp, message_id) VALUES (datetime('now'), ?)",);
        let _inserted = self.connection.execute(&insert_statement, [message_id])?;

        let delete_statement = format!(
            "DELETE FROM g{chat_id} WHERE id NOT IN (
                SELECT id FROM g{chat_id} ORDER BY id DESC LIMIT ?
            )",
        );
        let _removed = self
            .connection
            .execute(&delete_statement, [consts::MESSAGE_TO_STORE])?;

        Ok(())
    }
}
