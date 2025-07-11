from typing import List, Dict, Optional
from datetime import datetime
from .message import Message


class MessageThread:
    def __init__(self, subject: str):
        self.subject = subject
        self.messages: List[Message] = []
        self.participants: Dict[str, str] = {}  # email -> name mapping
        self.latest_date: Optional[datetime] = None
        self.earliest_date: Optional[datetime] = None
        self.unread_count = 0
        self.has_attachments = False
        self.is_flagged = False

    def add_message(self, message: Message):
        """Add a message to the thread and update thread metadata"""
        self.messages.append(message)

        # Update participants
        if message.sender['email']:
            self.participants[message.sender['email']] = message.sender['name'] or message.sender['email']

        for recipient in message.recipients:
            if recipient['email']:
                self.participants[recipient['email']] = recipient['name'] or recipient['email']

        # Update dates
        if message.date:
            if not self.latest_date or message.date > self.latest_date:
                self.latest_date = message.date
            if not self.earliest_date or message.date < self.earliest_date:
                self.earliest_date = message.date

        # Update flags
        if not message.is_read:
            self.unread_count += 1

        if message.has_attachments:
            self.has_attachments = True

        if message.is_flagged:
            self.is_flagged = True

        # Sort messages by date
        self.messages.sort(key=lambda m: m.date or datetime.min)

    def get_display_subject(self) -> str:
        """Get subject for display in thread list"""
        return self.subject or '(No Subject)'

    def get_display_sender(self) -> str:
        """Get sender info for display in thread list"""
        if not self.messages:
            return 'Unknown'

        # Use the latest message's sender
        latest_message = self.messages[-1]
        return latest_message.display_sender

    def get_display_date(self) -> str:
        """Get date for display in thread list"""
        if not self.latest_date:
            return ''

        # Use the same formatting as individual messages
        if self.messages:
            return self.messages[-1].display_date
        return ''

    def get_participant_summary(self) -> str:
        """Get summary of thread participants"""
        if len(self.participants) <= 1:
            return self.get_display_sender()
        elif len(self.participants) == 2:
            names = [name for name in self.participants.values() if name]
            return ' and '.join(names[:2])
        else:
            names = [name for name in self.participants.values() if name]
            return f"{names[0]} and {len(self.participants) - 1} others"

    def get_unread_count(self) -> int:
        """Get number of unread messages in thread"""
        return self.unread_count

    def __str__(self) -> str:
        return f"MessageThread(subject='{self.subject}', messages={len(self.messages)}, unread={self.unread_count})"
