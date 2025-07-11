from typing import List, Dict, Optional, Union, Any
from datetime import datetime
from .message import Message
import email.utils
import re


class MessageThread:
    def __init__(self, subject: str):
        self.subject = subject
        self.messages: List[Union[Message, Dict[str, Any]]] = []
        self.participants: Dict[str, str] = {}  # email -> name mapping
        self.latest_date: Optional[datetime] = None
        self.earliest_date: Optional[datetime] = None
        self.unread_count = 0
        self.has_attachments = False
        self.is_flagged = False

    def add_message(self, message: Union[Message, Dict[str, Any]]):
        """Add a message to the thread and update thread metadata"""
        self.messages.append(message)

        # Get message attributes regardless of format
        sender = self._get_attr(message, 'sender', {})
        recipients = self._get_attr(message, 'recipients', [])
        msg_date = self._get_attr(message, 'date', None)
        is_read = self._get_attr(message, 'is_read', True)
        has_attachments = self._get_attr(message, 'has_attachments', False)
        is_flagged = self._get_attr(message, 'is_flagged', False)

        # Update participants
        if isinstance(sender, dict) and sender.get('email'):
            self.participants[sender['email']] = sender.get('name') or sender['email']

        for recipient in recipients:
            if isinstance(recipient, dict) and recipient.get('email'):
                self.participants[recipient['email']] = recipient.get('name') or recipient['email']

        # Parse date if it's a string
        if isinstance(msg_date, str) and msg_date:
            try:
                parsed_date = email.utils.parsedate_tz(msg_date)
                if parsed_date:
                    timestamp = email.utils.mktime_tz(parsed_date)
                    msg_date = datetime.fromtimestamp(timestamp)
            except:
                msg_date = None

        # Update dates
        if msg_date:
            if not self.latest_date or msg_date > self.latest_date:
                self.latest_date = msg_date
            if not self.earliest_date or msg_date < self.earliest_date:
                self.earliest_date = msg_date

        # Update flags
        if not is_read:
            self.unread_count += 1

        if has_attachments:
            self.has_attachments = True

        if is_flagged:
            self.is_flagged = True

        # Sort messages by date
        self.messages.sort(key=lambda m: self._get_date_for_sort(m))

    def get_display_subject(self) -> str:
        """Get subject for display in thread list"""
        return self.subject or '(No Subject)'

    def get_display_sender(self) -> str:
        """Get sender info for display in thread list"""
        if not self.messages:
            return 'Unknown'

        # Use the latest message's sender
        latest_message = self.messages[-1]
        if isinstance(latest_message, dict):
            sender = latest_message.get('sender', {})
            if sender.get('name'):
                return sender['name']
            elif sender.get('email'):
                return sender['email']
            else:
                return 'Unknown Sender'
        else:
            return latest_message.display_sender

    def get_display_date(self) -> str:
        """Get date for display in thread list"""
        if not self.latest_date:
            return ''

        # Format date for display
        now = datetime.now()
        diff = now - self.latest_date

        if diff.days == 0:
            return self.latest_date.strftime('%H:%M')
        elif diff.days == 1:
            return 'Yesterday'
        elif diff.days < 7:
            return self.latest_date.strftime('%A')
        elif diff.days < 365:
            return self.latest_date.strftime('%b %d')
        else:
            return self.latest_date.strftime('%b %d, %Y')

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

    def _get_attr(self, message: Union[Message, Dict[str, Any]], attr: str, default=None):
        """Get attribute from message (works with both Message objects and dicts)"""
        if isinstance(message, dict):
            return message.get(attr, default)
        else:
            return getattr(message, attr, default)

    def _get_date_for_sort(self, message: Union[Message, Dict[str, Any]]) -> datetime:
        """Get date for sorting messages"""
        msg_date = self._get_attr(message, 'date', None)

        if isinstance(msg_date, str) and msg_date:
            try:
                parsed_date = email.utils.parsedate_tz(msg_date)
                if parsed_date:
                    timestamp = email.utils.mktime_tz(parsed_date)
                    return datetime.fromtimestamp(timestamp)
            except:
                pass
        elif isinstance(msg_date, datetime):
            return msg_date

        return datetime.min

    def __str__(self) -> str:
        return f"MessageThread(subject='{self.subject}', messages={len(self.messages)}, unread={self.unread_count})"
