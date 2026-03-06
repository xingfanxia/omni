-- Add parent_id column to chat_messages to support message branching/editing
ALTER TABLE chat_messages ADD COLUMN parent_id VARCHAR(26) REFERENCES chat_messages(id) ON DELETE SET NULL;

CREATE INDEX idx_chat_messages_parent_id ON chat_messages(parent_id);

-- Backfill existing messages: set each message's parent_id to the previous message in the same chat
WITH ordered AS (
    SELECT id, chat_id, message_seq_num,
           LAG(id) OVER (PARTITION BY chat_id ORDER BY message_seq_num) AS prev_id
    FROM chat_messages
)
UPDATE chat_messages cm
SET parent_id = ordered.prev_id
FROM ordered
WHERE cm.id = ordered.id
  AND ordered.prev_id IS NOT NULL;
