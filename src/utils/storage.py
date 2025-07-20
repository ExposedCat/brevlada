import sqlite3
import json
import threading
import logging
from datetime import datetime, timedelta
from typing import List, Dict, Optional
from pathlib import Path


class EmailStorage:
    def __init__(self, db_path: Optional[str] = None):
        if db_path is None:
            # Default to user's cache directory
            cache_dir = Path.home() / ".cache" / "brevlada"
            cache_dir.mkdir(parents=True, exist_ok=True)
            db_path = str(cache_dir / "emails.db")

        self.db_path = db_path
        logging.debug(f"EmailStorage: Initializing with database path: {db_path}")
        self._lock = threading.Lock()
        self._init_database()

    def get_connection(self):
        """Get database connection"""
        logging.debug(f"EmailStorage: Opening database connection to {self.db_path}")
        conn = sqlite3.connect(self.db_path)
        conn.row_factory = sqlite3.Row
        return conn

    def _init_database(self):
        """Initialize database schema"""
        logging.debug("EmailStorage: Initializing database schema")
        with self.get_connection() as conn:
            # Messages table
            conn.execute(
                """
                CREATE TABLE IF NOT EXISTS messages (
                    uid INTEGER PRIMARY KEY,
                    folder TEXT NOT NULL,
                    account_id TEXT NOT NULL,
                    message_id TEXT,
                    subject TEXT,
                    sender_name TEXT,
                    sender_email TEXT,
                    recipients TEXT, -- JSON array
                    cc TEXT, -- JSON array
                    bcc TEXT, -- JSON array
                    reply_to TEXT, -- JSON array
                    date_sent TIMESTAMP,
                    date_received TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    flags TEXT, -- JSON array
                    is_read BOOLEAN DEFAULT 0,
                    is_flagged BOOLEAN DEFAULT 0,
                    is_deleted BOOLEAN DEFAULT 0,
                    is_draft BOOLEAN DEFAULT 0,
                    is_answered BOOLEAN DEFAULT 0,
                    has_attachments BOOLEAN DEFAULT 0,
                    body_text TEXT,
                    body_html TEXT,
                    headers TEXT, -- JSON object
                    envelope TEXT, -- JSON object
                    bodystructure TEXT, -- JSON object
                    thread_subject TEXT,
                    thread_references TEXT, -- JSON array
                    in_reply_to TEXT,
                    message_references TEXT,
                    sync_status TEXT DEFAULT 'pending',
                    last_sync TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UNIQUE(uid, folder, account_id)
                )
            """
            )

            # Threads table
            conn.execute(
                """
                CREATE TABLE IF NOT EXISTS threads (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    subject TEXT NOT NULL,
                    folder TEXT NOT NULL,
                    account_id TEXT NOT NULL,
                    message_count INTEGER DEFAULT 0,
                    unread_count INTEGER DEFAULT 0,
                    has_attachments BOOLEAN DEFAULT 0,
                    is_flagged BOOLEAN DEFAULT 0,
                    participants TEXT, -- JSON object
                    latest_date TIMESTAMP,
                    earliest_date TIMESTAMP,
                    latest_message_uid INTEGER,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UNIQUE(subject, folder, account_id)
                )
            """
            )

            # Attachments table
            conn.execute(
                """
                CREATE TABLE IF NOT EXISTS attachments (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    message_uid INTEGER NOT NULL,
                    folder TEXT NOT NULL,
                    account_id TEXT NOT NULL,
                    filename TEXT NOT NULL,
                    content_type TEXT,
                    size INTEGER,
                    part_id TEXT,
                    is_inline BOOLEAN DEFAULT 0,
                    content_id TEXT,
                    downloaded BOOLEAN DEFAULT 0,
                    file_path TEXT,
                    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    FOREIGN KEY (message_uid) REFERENCES messages (uid)
                )
            """
            )

            # Sync status table
            conn.execute(
                """
                CREATE TABLE IF NOT EXISTS sync_status (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    account_id TEXT NOT NULL,
                    folder TEXT NOT NULL,
                    last_sync TIMESTAMP,
                    last_uid INTEGER,
                    total_messages INTEGER DEFAULT 0,
                    sync_errors INTEGER DEFAULT 0,
                    status TEXT DEFAULT 'idle',
                    UNIQUE(account_id, folder)
                )
            """
            )

            # Create indexes for performance
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_messages_folder_account ON messages(folder, account_id)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_messages_date ON messages(date_sent)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_messages_thread ON messages(thread_subject, folder, account_id)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_messages_flags ON messages(is_read, is_flagged)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_messages_search ON messages(subject, sender_name, sender_email)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_threads_folder_account ON threads(folder, account_id)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_threads_date ON threads(latest_date)"
            )
            conn.execute(
                "CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments(message_uid)"
            )

            logging.debug("EmailStorage: Database schema initialization complete")

    def store_account(self, account_id: str, account_data: Dict):
        """Store account information"""
        # This could be expanded to store account metadata
        pass

    def store_folder(self, account_id: str, folder: str, folder_data: Dict):
        """Store folder information"""
        # This could be expanded to store folder metadata
        pass

    def store_messages(self, messages: List, folder: str, account_id: str):
        """Store multiple messages"""
        logging.debug(
            f"EmailStorage: Storing {len(messages)} messages for folder '{folder}', account_id '{account_id}'"
        )
        with self._lock:
            for i, message in enumerate(messages):
                logging.debug(f"EmailStorage: Storing message {i+1}/{len(messages)}")
                self.store_message(message, folder, account_id)

    def store_message(self, message, folder: str, account_id: str):
        """Store a single message"""
        # Handle both dictionary and object formats
        if isinstance(message, dict):
            uid = message.get("uid")
            message_id = message.get("message_id", "")
            subject = message.get("subject", "")
            sender = message.get("sender", {})
            sender_name = sender.get("name", "") if sender else ""
            sender_email = sender.get("email", "") if sender else ""
            recipients = message.get("recipients", [])
            cc = message.get("cc", [])
            bcc = message.get("bcc", [])
            reply_to = message.get("reply_to", [])
            date_sent = message.get("date", "")
            if date_sent is None:
                date_sent = ""
            flags = message.get("flags", [])
            is_read = message.get("is_read", False)
            is_flagged = message.get("is_flagged", False)
            is_deleted = message.get("is_deleted", False)
            is_draft = message.get("is_draft", False)
            is_answered = message.get("is_answered", False)
            has_attachments = message.get("has_attachments", False)
            body_text = message.get("body", "")
            body_html = message.get("body_html", "")
            headers = message.get("headers", {})
            envelope = message.get("envelope", {})
            bodystructure = message.get("bodystructure", {})
            thread_subject = message.get("thread_subject", "")
            thread_references = message.get("thread_references", [])
            in_reply_to = message.get("in_reply_to", "")
            references = message.get("references", "")
            attachments = message.get("attachments", [])
        else:
            # Object format
            uid = message.uid
            message_id = message.message_id
            subject = message.subject
            sender_name = message.sender["name"]
            sender_email = message.sender["email"]
            recipients = message.recipients
            cc = message.cc
            bcc = message.bcc
            reply_to = message.reply_to
            date_sent = message.date
            if date_sent is not None:
                date_sent = (
                    date_sent.isoformat()
                    if hasattr(date_sent, "isoformat")
                    else str(date_sent)
                )
            else:
                date_sent = ""
            flags = message.flags
            is_read = message.is_read
            is_flagged = message.is_flagged
            is_deleted = message.is_deleted
            is_draft = message.is_draft
            is_answered = message.is_answered
            has_attachments = message.has_attachments
            body_text = message.body
            body_html = None
            headers = message.raw_headers
            envelope = message.envelope
            bodystructure = message.body_structure
            thread_subject = message.thread_subject
            thread_references = message.thread_references
            in_reply_to = message.in_reply_to
            references = message.references
            attachments = message.attachments

        logging.debug(f"EmailStorage: Storing message uid={uid} for folder '{folder}'")
        try:
            with self.get_connection() as conn:
                conn.execute(
                    """
                    INSERT OR REPLACE INTO messages (
                        uid, folder, account_id, message_id, subject,
                        sender_name, sender_email, recipients, cc, bcc, reply_to,
                        date_sent, flags, is_read, is_flagged, is_deleted, is_draft, is_answered,
                        has_attachments, body_text, body_html, headers, envelope, bodystructure,
                        thread_subject, thread_references, in_reply_to, message_references,
                        sync_status, last_sync
                    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                """,
                    (
                        uid,
                        folder,
                        account_id,
                        message_id,
                        subject,
                        sender_name,
                        sender_email,
                        json.dumps(recipients),
                        json.dumps(cc),
                        json.dumps(bcc),
                        json.dumps(reply_to),
                        date_sent,
                        json.dumps(flags),
                        is_read,
                        is_flagged,
                        is_deleted,
                        is_draft,
                        is_answered,
                        has_attachments,
                        body_text,
                        body_html,
                        json.dumps(headers),
                        json.dumps(envelope),
                        json.dumps(bodystructure),
                        thread_subject,
                        json.dumps(thread_references),
                        in_reply_to,
                        references,
                        "synced",
                        datetime.now(),
                    ),
                )

                # Store attachments
                if uid is not None:
                    for attachment in attachments:
                        if isinstance(attachment, dict):
                            self.store_attachment_dict(
                                attachment, uid, folder, account_id
                            )
                        else:
                            self.store_attachment(attachment, uid, folder, account_id)

                logging.debug(f"EmailStorage: Successfully stored message uid={uid}")
        except Exception as e:
            logging.error(f"EmailStorage: Error storing message uid={uid}: {e}")
            raise

    def store_attachment(
        self, attachment, message_uid: int, folder: str, account_id: str
    ):
        """Store attachment information"""
        with self.get_connection() as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO attachments (
                    message_uid, folder, account_id, filename, content_type,
                    size, part_id, is_inline, content_id
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    message_uid,
                    folder,
                    account_id,
                    attachment.filename,
                    attachment.content_type,
                    attachment.size,
                    attachment.part_id,
                    attachment.is_inline,
                    attachment.content_id,
                ),
            )

    def store_attachment_dict(
        self, attachment_dict, message_uid: int, folder: str, account_id: str
    ):
        """Store attachment information from dictionary"""
        with self.get_connection() as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO attachments (
                    message_uid, folder, account_id, filename, content_type,
                    size, part_id, is_inline, content_id
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """,
                (
                    message_uid,
                    folder,
                    account_id,
                    attachment_dict.get("filename", ""),
                    attachment_dict.get("content_type", ""),
                    attachment_dict.get("size", 0),
                    attachment_dict.get("part_id", ""),
                    attachment_dict.get("is_inline", False),
                    attachment_dict.get("content_id", ""),
                ),
            )

    def get_messages(
        self,
        folder: str,
        account_id: Optional[str] = None,
        limit: int = 50,
        offset: int = 0,
    ) -> List[Dict]:
        """Get messages from storage"""
        logging.debug(
            f"EmailStorage: Getting messages for folder='{folder}', account_id='{account_id}', limit={limit}, offset={offset}"
        )

        # If account_id is None, we need to handle this case
        if account_id is None:
            logging.warning("EmailStorage: account_id is None, returning empty list")
            return []

        try:
            with self.get_connection() as conn:
                logging.debug(
                    f"EmailStorage: Executing query for messages in folder '{folder}'"
                )
                cursor = conn.execute(
                    """
                    SELECT * FROM messages
                    WHERE folder = ? AND account_id = ? AND is_deleted = 0
                    ORDER BY date_sent DESC
                    LIMIT ? OFFSET ?
                """,
                    (folder, account_id, limit, offset),
                )

                rows = cursor.fetchall()
                logging.debug(f"EmailStorage: Query returned {len(rows)} rows")

                messages = []
                for i, row in enumerate(rows):
                    logging.debug(f"EmailStorage: Processing message {i+1}/{len(rows)}")
                    message_data = self._row_to_message(row)
                    # Get attachments
                    message_data["attachments"] = self.get_message_attachments(
                        row["uid"], folder, account_id
                    )
                    messages.append(message_data)

                logging.info(
                    f"EmailStorage: Successfully retrieved {len(messages)} messages for folder '{folder}'"
                )
                return messages
        except Exception as e:
            logging.error(
                f"EmailStorage: Error getting messages for folder '{folder}': {e}"
            )
            logging.debug(
                f"EmailStorage: Exception details: {type(e).__name__}: {str(e)}"
            )
            raise

    def _row_to_message(self, row: sqlite3.Row) -> Dict:
        """Convert database row to message dictionary"""
        logging.debug(f"EmailStorage: Converting row to message for uid={row['uid']}")
        return {
            "uid": row["uid"],
            "folder": row["folder"],
            "account_id": row["account_id"],
            "message_id": row["message_id"],
            "subject": row["subject"],
            "sender": {"name": row["sender_name"], "email": row["sender_email"]},
            "recipients": json.loads(row["recipients"] or "[]"),
            "cc": json.loads(row["cc"] or "[]"),
            "bcc": json.loads(row["bcc"] or "[]"),
            "reply_to": json.loads(row["reply_to"] or "[]"),
            "date": row["date_sent"],
            "flags": json.loads(row["flags"] or "[]"),
            "is_read": bool(row["is_read"]),
            "is_flagged": bool(row["is_flagged"]),
            "is_deleted": bool(row["is_deleted"]),
            "is_draft": bool(row["is_draft"]),
            "is_answered": bool(row["is_answered"]),
            "has_attachments": bool(row["has_attachments"]),
            "body": row["body_text"],
            "body_html": row["body_html"],
            "headers": json.loads(row["headers"] or "{}"),
            "envelope": json.loads(row["envelope"] or "{}"),
            "bodystructure": json.loads(row["bodystructure"] or "{}"),
            "thread_subject": row["thread_subject"],
            "thread_references": json.loads(row["thread_references"] or "[]"),
            "in_reply_to": row["in_reply_to"],
            "references": row["message_references"],
        }

    def get_message_attachments(
        self, message_uid: int, folder: str, account_id: str
    ) -> List[Dict]:
        """Get attachments for a specific message"""
        logging.debug(
            f"EmailStorage: Getting attachments for message uid={message_uid}"
        )
        try:
            with self.get_connection() as conn:
                cursor = conn.execute(
                    """
                    SELECT * FROM attachments
                    WHERE message_uid = ? AND folder = ? AND account_id = ?
                """,
                    (message_uid, folder, account_id),
                )

                attachments = []
                for row in cursor.fetchall():
                    attachments.append(
                        {
                            "id": row["id"],
                            "filename": row["filename"],
                            "content_type": row["content_type"],
                            "size": row["size"],
                            "part_id": row["part_id"],
                            "is_inline": bool(row["is_inline"]),
                            "content_id": row["content_id"],
                            "downloaded": bool(row["downloaded"]),
                            "file_path": row["file_path"],
                        }
                    )

                logging.debug(
                    f"EmailStorage: Found {len(attachments)} attachments for message uid={message_uid}"
                )
                return attachments
        except Exception as e:
            logging.error(
                f"EmailStorage: Error getting attachments for message uid={message_uid}: {e}"
            )
            return []

    def search_messages(
        self, query: str, folder: str, account_id: str, limit: int = 50
    ) -> List[Dict]:
        """Search messages by query"""
        with self.get_connection() as conn:
            search_query = f"%{query}%"
            cursor = conn.execute(
                """
                SELECT * FROM messages
                WHERE folder = ? AND account_id = ? AND is_deleted = 0
                AND (subject LIKE ? OR sender_name LIKE ? OR sender_email LIKE ? OR body_text LIKE ?)
                ORDER BY date_sent DESC
                LIMIT ?
            """,
                (
                    folder,
                    account_id,
                    search_query,
                    search_query,
                    search_query,
                    search_query,
                    limit,
                ),
            )

            messages = []
            for row in cursor.fetchall():
                message_data = self._row_to_message(row)
                message_data["attachments"] = self.get_message_attachments(
                    row["uid"], folder, account_id
                )
                messages.append(message_data)

            return messages

    def update_sync_status(
        self, account_id: str, folder: str, status: str, last_uid: Optional[int] = None
    ):
        """Update sync status for folder"""
        with self.get_connection() as conn:
            conn.execute(
                """
                INSERT OR REPLACE INTO sync_status (
                    account_id, folder, last_sync, last_uid, status
                ) VALUES (?, ?, ?, ?, ?)
            """,
                (account_id, folder, datetime.now(), last_uid, status),
            )

    def get_sync_status(self, account_id: str, folder: str) -> Optional[Dict]:
        """Get sync status for folder"""
        with self.get_connection() as conn:
            cursor = conn.execute(
                """
                SELECT * FROM sync_status
                WHERE account_id = ? AND folder = ?
            """,
                (account_id, folder),
            )

            row = cursor.fetchone()
            if row:
                return {
                    "account_id": row["account_id"],
                    "folder": row["folder"],
                    "last_sync": row["last_sync"],
                    "last_uid": row["last_uid"],
                    "total_messages": row["total_messages"],
                    "sync_errors": row["sync_errors"],
                    "status": row["status"],
                }
            return None

    def cleanup_old_messages(self, days_old: int = 30):
        """Clean up old messages"""
        cutoff_date = datetime.now() - timedelta(days=days_old)

        with self.get_connection() as conn:
            # Delete old messages
            conn.execute(
                """
                DELETE FROM messages
                WHERE date_sent < ? AND is_flagged = 0
            """,
                (cutoff_date,),
            )

            # Delete orphaned attachments
            conn.execute(
                """
                DELETE FROM attachments
                WHERE message_uid NOT IN (SELECT uid FROM messages)
            """
            )

            # Vacuum database
            conn.execute("VACUUM")

    def update_message_body(self, uid: int, folder: str, account_id: str, body_text: str, body_html: str = None):
        """Update message body in database"""
        logging.debug(f"EmailStorage: Updating message body for uid={uid}")
        try:
            with self.get_connection() as conn:
                if body_html is not None:
                    conn.execute(
                        """
                        UPDATE messages
                        SET body_text = ?, body_html = ?
                        WHERE uid = ? AND folder = ? AND account_id = ?
                    """,
                        (body_text, body_html, uid, folder, account_id),
                    )
                else:
                    conn.execute(
                        """
                        UPDATE messages
                        SET body_text = ?
                        WHERE uid = ? AND folder = ? AND account_id = ?
                    """,
                        (body_text, uid, folder, account_id),
                    )
                logging.debug(f"EmailStorage: Successfully updated message body for uid={uid}")
        except Exception as e:
            logging.error(f"EmailStorage: Error updating message body for uid={uid}: {e}")
            raise
