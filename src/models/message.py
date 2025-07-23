import email
import email.utils
import email.header
import re
from datetime import datetime
from typing import List, Dict, Optional, Any

class Message:
    def __init__(self, message_data: Dict[str, Any]):
        self.uid = message_data.get("uid")
        self.flags = message_data.get("flags", [])
        self.envelope = message_data.get("envelope", {})
        self.body_structure = message_data.get("bodystructure")
        self.raw_headers = message_data.get("headers", "")
        self.body = message_data.get("body", "")

        
        self.subject = self._decode_header(self.envelope.get("subject", ""))
        self.sender = self._parse_address(self.envelope.get("from", []))
        self.recipients = self._parse_address_list(self.envelope.get("to", []))
        self.cc = self._parse_address_list(self.envelope.get("cc", []))
        self.bcc = self._parse_address_list(self.envelope.get("bcc", []))
        self.reply_to = self._parse_address_list(self.envelope.get("reply_to", []))
        self.date = self._parse_date(self.envelope.get("date"))
        self.message_id = self.envelope.get("message_id", "")
        self.in_reply_to = self.envelope.get("in_reply_to", "")
        self.references = self.envelope.get("references", "")

        
        self.is_read = "\\Seen" in self.flags
        self.is_flagged = "\\Flagged" in self.flags
        self.is_deleted = "\\Deleted" in self.flags
        self.is_draft = "\\Draft" in self.flags
        self.is_answered = "\\Answered" in self.flags

        
        self.thread_subject = self.get_thread_subject()
        self.thread_references = self._parse_references()

        
        self.has_attachments = self._has_attachments()
        self.attachments = []

        
        self.display_sender = self._format_sender_for_display()
        self.display_subject = self._format_subject_for_display()
        self.display_date = self._format_date_for_display()

    def _decode_header(self, header_value: str) -> str:
        """Decode email header value handling MIME encoding"""
        if not header_value:
            return ""

        try:
            decoded_parts = email.header.decode_header(header_value)
            decoded_string = ""

            for part, encoding in decoded_parts:
                if isinstance(part, bytes):
                    if encoding:
                        decoded_string += part.decode(encoding)
                    else:
                        decoded_string += part.decode("utf-8", errors="ignore")
                else:
                    decoded_string += str(part)

            return decoded_string.strip()
        except Exception:
            return str(header_value)

    def _parse_address(self, address_data: List) -> Dict[str, str]:
        """Parse a single address from envelope data"""
        if not address_data or not isinstance(address_data, list):
            return {"name": "", "email": ""}

        try:
            addr = address_data[0] if address_data else {}
            name = self._decode_header(addr.get("name", ""))
            mailbox = addr.get("mailbox", "")
            host = addr.get("host", "")
            email_addr = f"{mailbox}@{host}" if mailbox and host else ""

            return {"name": name, "email": email_addr}
        except Exception:
            return {"name": "", "email": ""}

    def _parse_address_list(self, address_list: List) -> List[Dict[str, str]]:
        """Parse a list of addresses from envelope data"""
        if not address_list:
            return []

        addresses = []
        for addr_data in address_list:
            if isinstance(addr_data, dict):
                addr = self._parse_address([addr_data])
                if addr["email"]:
                    addresses.append(addr)

        return addresses

    def _parse_date(self, date_str: str) -> Optional[datetime]:
        """Parse date string into datetime object"""
        if not date_str:
            return None

        try:
            
            parsed_date = email.utils.parsedate_tz(date_str)
            if parsed_date:
                timestamp = email.utils.mktime_tz(parsed_date)
                return datetime.fromtimestamp(timestamp)
        except Exception:
            pass

        return None

    def _format_date_for_display(self) -> str:
        """Format date for display in message list"""
        if not self.date:
            return ""

        now = datetime.now()
        diff = now - self.date

        if diff.days == 0:
            
            return self.date.strftime("%H:%M")
        elif diff.days == 1:
            
            return "Yesterday"
        elif diff.days < 7:
            
            return self.date.strftime("%A")
        elif diff.days < 365:
            
            return self.date.strftime("%b %d")
        else:
            
            return self.date.strftime("%b %d, %Y")

    def _format_sender_for_display(self) -> str:
        """Format sender for display in message list"""
        if self.sender["name"]:
            return self.sender["name"]
        elif self.sender["email"]:
            return self.sender["email"]
        else:
            return "Unknown Sender"

    def _format_subject_for_display(self) -> str:
        """Format subject for display in message list"""
        if self.subject:
            return self.subject
        else:
            return "(No Subject)"

    def get_thread_subject(self) -> str:
        """Get normalized subject for threading"""
        if not self.subject:
            return ""

        
        normalized = re.sub(
            r"^(Re|Fwd?|AW|Antw|回复|转发):\s*", "", self.subject, flags=re.IGNORECASE
        )
        normalized = re.sub(r"\s+", " ", normalized).strip()

        return normalized

    def _parse_references(self) -> List[str]:
        """Parse References header for threading"""
        refs = []

        if self.references:
            
            ref_ids = re.findall(r"<([^>]+)>", self.references)
            refs.extend(ref_ids)

        if self.in_reply_to:
            ref_ids = re.findall(r"<([^>]+)>", self.in_reply_to)
            refs.extend(ref_ids)

        return refs

    def _has_attachments(self) -> bool:
        """Check if message has attachments based on body structure"""
        if not self.body_structure:
            return False

        
        if isinstance(self.body_structure, list) and len(self.body_structure) > 1:
            return True

        return False

    def add_attachment(self, attachment):
        """Add attachment to message"""
        self.attachments.append(attachment)
        self.has_attachments = True

    def get_attachment_count(self) -> int:
        """Get number of attachments"""
        return len(self.attachments)

    def get_attachment_summary(self) -> str:
        """Get summary of attachments for display"""
        count = self.get_attachment_count()
        if count == 0:
            return ""
        elif count == 1:
            return "1 attachment"
        else:
            return f"{count} attachments"

    def get_display_sender(self) -> str:
        """Get formatted sender for display"""
        return self.display_sender

    def get_display_subject(self) -> str:
        """Get formatted subject for display"""
        return self.display_subject

    def get_display_date(self) -> str:
        """Get formatted date for display"""
        return self.display_date

    def __str__(self) -> str:
        return f"Message(uid={self.uid}, subject='{self.subject}', sender='{self.display_sender}')"

class Attachment:
    def __init__(
        self,
        filename: str,
        content_type: Optional[str] = None,
        size: Optional[int] = None,
        part_id: Optional[str] = None,
        is_inline: bool = False,
        content_id: Optional[str] = None,
    ):
        self.filename = filename
        self.content_type = content_type
        self.size = size
        self.part_id = part_id
        self.is_inline = is_inline
        self.content_id = content_id
        self.downloaded = False
        self.file_path = None

    def get_display_name(self) -> str:
        """Get filename for display"""
        return self.filename or "Unknown"

    def get_file_extension(self) -> str:
        """Get file extension"""
        if self.filename and "." in self.filename:
            return self.filename.rsplit(".", 1)[1].lower()
        return ""

    def get_icon_name(self) -> str:
        """Get appropriate icon name based on file type"""
        ext = self.get_file_extension()

        if ext in ["jpg", "jpeg", "png", "gif", "bmp", "svg"]:
            return "image-x-generic"
        elif ext in ["pdf"]:
            return "application-pdf"
        elif ext in ["doc", "docx"]:
            return "application-msword"
        elif ext in ["xls", "xlsx"]:
            return "application-vnd.ms-excel"
        elif ext in ["ppt", "pptx"]:
            return "application-vnd.ms-powerpoint"
        elif ext in ["txt", "md"]:
            return "text-x-generic"
        elif ext in ["zip", "rar", "7z", "tar", "gz"]:
            return "application-x-archive"
        else:
            return "text-x-generic"

    def get_size_string(self) -> str:
        """Get human-readable file size"""
        if not self.size:
            return "Unknown size"

        if self.size < 1024:
            return f"{self.size} B"
        elif self.size < 1024 * 1024:
            return f"{self.size / 1024:.1f} KB"
        elif self.size < 1024 * 1024 * 1024:
            return f"{self.size / (1024 * 1024):.1f} MB"
        else:
            return f"{self.size / (1024 * 1024 * 1024):.1f} GB"
