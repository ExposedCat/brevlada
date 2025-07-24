import email
import email.utils
import email.header
from typing import Dict, List, Optional, Any, Tuple
import re
from models.message import Message

def parse_message_from_imap(uid: int, fetch_data: Tuple) -> Optional[Message]:
    """Parse IMAP fetch response into Message object"""
    if not fetch_data or len(fetch_data) < 2:
        return None

    try:
        message_data = {}
        message_data["uid"] = uid

        for item in fetch_data:
            if isinstance(item, tuple) and len(item) == 2:
                key, value = item
                if isinstance(key, bytes):
                    key = key.decode("utf-8")

                if key.startswith("ENVELOPE"):
                    message_data["envelope"] = parse_envelope(value)
                elif key.startswith("BODYSTRUCTURE"):
                    message_data["bodystructure"] = parse_bodystructure(value)
                elif key.startswith("FLAGS"):
                    message_data["flags"] = parse_flags(value)
                elif key.startswith("BODY[HEADER"):
                    message_data["headers"] = value.decode("utf-8", errors="ignore")
                elif key.startswith("BODY[TEXT"):
                    message_data["body"] = value.decode("utf-8", errors="ignore")

        return Message(message_data)
    except Exception:
        return None

def parse_envelope(envelope_data) -> Dict[str, Any]:
    """Parse IMAP envelope data"""
    if not envelope_data:
        return {}

    try:
        if isinstance(envelope_data, bytes):
            envelope_str = envelope_data.decode("utf-8", errors="ignore")
        else:
            envelope_str = str(envelope_data)

        envelope = {}

        parts = parse_envelope_parts(envelope_str)
        if len(parts) >= 10:
            envelope["date"] = decode_envelope_field(parts[0])
            envelope["subject"] = decode_envelope_field(parts[1])
            envelope["from"] = parse_address_list(parts[2])
            envelope["sender"] = parse_address_list(parts[3])
            envelope["reply_to"] = parse_address_list(parts[4])
            envelope["to"] = parse_address_list(parts[5])
            envelope["cc"] = parse_address_list(parts[6])
            envelope["bcc"] = parse_address_list(parts[7])
            envelope["in_reply_to"] = decode_envelope_field(parts[8])
            envelope["message_id"] = decode_envelope_field(parts[9])

        return envelope
    except Exception:
        return {}

def parse_envelope_parts(envelope_str: str) -> List[str]:
    """Parse envelope string into parts"""
    if not envelope_str.startswith("(") or not envelope_str.endswith(")"):
        return []

    parts = []
    current = ""
    depth = 0
    in_quotes = False
    escape_next = False

    for char in envelope_str[1:-1]:
        if escape_next:
            current += char
            escape_next = False
            continue

        if char == "\\":
            escape_next = True
            current += char
            continue

        if char == '"' and not escape_next:
            in_quotes = not in_quotes
            current += char
            continue

        if not in_quotes:
            if char == "(":
                depth += 1
            elif char == ")":
                depth -= 1
            elif char == " " and depth == 0:
                parts.append(current.strip())
                current = ""
                continue

        current += char

    if current.strip():
        parts.append(current.strip())

    return parts

def decode_envelope_field(field: str) -> str:
    """Decode envelope field value"""
    if not field or field == "NIL":
        return ""

    if field.startswith('"') and field.endswith('"'):
        field = field[1:-1]

    try:
        decoded_parts = email.header.decode_header(field)
        result = ""
        for part, encoding in decoded_parts:
            if isinstance(part, bytes):
                if encoding:
                    result += part.decode(encoding)
                else:
                    result += part.decode("utf-8", errors="ignore")
            else:
                result += str(part)
        return result.strip()
    except Exception:
        return field

def parse_address_list(addr_str: str) -> List[Dict[str, str]]:
    """Parse address list from envelope"""
    if not addr_str or addr_str == "NIL":
        return []

    addresses = []

    if addr_str.startswith("(") and addr_str.endswith(")"):
        addr_parts = parse_address_parts(addr_str[1:-1])

        for part in addr_parts:
            if part.startswith("(") and part.endswith(")"):
                addr_fields = parse_envelope_parts(part)
                if len(addr_fields) >= 4:
                    name = decode_envelope_field(addr_fields[0])
                    mailbox = decode_envelope_field(addr_fields[2])
                    host = decode_envelope_field(addr_fields[3])

                    email_addr = f"{mailbox}@{host}" if mailbox and host else ""
                    if email_addr:
                        addresses.append({"name": name, "email": email_addr})

    return addresses

def parse_address_parts(addr_str: str) -> List[str]:
    """Parse address string into individual address parts"""
    parts = []
    current = ""
    depth = 0

    for char in addr_str:
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
            current += char
            if depth == 0:
                parts.append(current.strip())
                current = ""
                continue

        current += char

    if current.strip():
        parts.append(current.strip())

    return parts

def parse_bodystructure(bodystructure_data) -> Dict[str, Any]:
    """Parse IMAP bodystructure data"""
    if not bodystructure_data:
        return {}

    try:
        if isinstance(bodystructure_data, bytes):
            bs_str = bodystructure_data.decode("utf-8", errors="ignore")
        else:
            bs_str = str(bodystructure_data)

        return parse_bodystructure_string(bs_str)
    except Exception:
        return {}

def parse_bodystructure_string(bs_str: str) -> Dict[str, Any]:
    """Parse bodystructure string into structured data"""
    if not bs_str.startswith("(") or not bs_str.endswith(")"):
        return {}

    structure = {}
    parts = parse_envelope_parts(bs_str)

    if len(parts) >= 2:
        structure["type"] = decode_envelope_field(parts[0])
        structure["subtype"] = decode_envelope_field(parts[1])

        if len(parts) >= 7:
            structure["parameters"] = parse_bodystructure_parameters(parts[2])
            structure["id"] = decode_envelope_field(parts[3])
            structure["description"] = decode_envelope_field(parts[4])
            structure["encoding"] = decode_envelope_field(parts[5])
            structure["size"] = parse_bodystructure_size(parts[6])

        if structure["type"].lower() == "multipart":
            structure["parts"] = []

    return structure

def parse_bodystructure_parameters(param_str: str) -> Dict[str, str]:
    """Parse bodystructure parameters"""
    if not param_str or param_str == "NIL":
        return {}

    params = {}
    if param_str.startswith("(") and param_str.endswith(")"):
        param_parts = parse_envelope_parts(param_str[1:-1])

        for i in range(0, len(param_parts), 2):
            if i + 1 < len(param_parts):
                key = decode_envelope_field(param_parts[i])
                value = decode_envelope_field(param_parts[i + 1])
                params[key.lower()] = value

    return params

def parse_bodystructure_size(size_str: str) -> int:
    """Parse bodystructure size field"""
    try:
        return int(size_str)
    except (ValueError, TypeError):
        return 0

def parse_flags(flags_data) -> List[str]:
    """Parse IMAP flags"""
    if not flags_data:
        return []

    flags = []
    if isinstance(flags_data, bytes):
        flags_str = flags_data.decode("utf-8", errors="ignore")
    else:
        flags_str = str(flags_data)

    if flags_str.startswith("(") and flags_str.endswith(")"):
        flags_str = flags_str[1:-1]

    flag_parts = flags_str.split()
    for flag in flag_parts:
        flag = flag.strip()
        if flag.startswith("\\"):
            flags.append(flag)

    return flags

def create_message_from_raw_email(
    raw_email: str, uid: Optional[int] = None
) -> Optional[Message]:
    """Create Message object from raw email text"""
    try:
        email_msg = email.message_from_string(raw_email)

        message_data = {
            "uid": uid,
            "flags": [],
            "envelope": extract_envelope_from_email(email_msg),
            "bodystructure": extract_bodystructure_from_email(email_msg),
            "headers": raw_email.split("\n\n")[0],
            "body": extract_body_from_email(email_msg),
        }

        return Message(message_data)
    except Exception:
        return None

def extract_envelope_from_email(email_msg) -> Dict[str, Any]:
    """Extract envelope data from email.Message object"""
    envelope = {}

    envelope["date"] = email_msg.get("Date", "")
    envelope["subject"] = email_msg.get("Subject", "")
    envelope["message_id"] = email_msg.get("Message-ID", "")
    envelope["in_reply_to"] = email_msg.get("In-Reply-To", "")
    envelope["references"] = email_msg.get("References", "")

    from_addr = parse_email_address(email_msg.get("From", ""))
    envelope["from"] = [from_addr] if from_addr["email"] else []

    to_addrs = parse_email_address_list(email_msg.get("To", ""))
    envelope["to"] = to_addrs

    cc_addrs = parse_email_address_list(email_msg.get("Cc", ""))
    envelope["cc"] = cc_addrs

    bcc_addrs = parse_email_address_list(email_msg.get("Bcc", ""))
    envelope["bcc"] = bcc_addrs

    reply_to_addrs = parse_email_address_list(email_msg.get("Reply-To", ""))
    envelope["reply_to"] = reply_to_addrs

    envelope["sender"] = envelope["from"]

    return envelope

def parse_email_address(addr_str: str) -> Dict[str, str]:
    """Parse single email address"""
    if not addr_str:
        return {"name": "", "email": ""}

    try:
        name, email_addr = email.utils.parseaddr(addr_str)
        return {"name": name or "", "email": email_addr or ""}
    except Exception:
        return {"name": "", "email": addr_str}

def parse_email_address_list(addr_str: str) -> List[Dict[str, str]]:
    """Parse email address list"""
    if not addr_str:
        return []

    addresses = []
    try:
        addr_list = email.utils.getaddresses([addr_str])
        for name, email_addr in addr_list:
            if email_addr:
                addresses.append({"name": name or "", "email": email_addr})
    except Exception:
        pass

    return addresses

def extract_bodystructure_from_email(email_msg) -> Dict[str, Any]:
    """Extract bodystructure from email.Message object"""
    structure = {}

    content_type = email_msg.get_content_type()
    if "/" in content_type:
        main_type, sub_type = content_type.split("/", 1)
        structure["type"] = main_type
        structure["subtype"] = sub_type

    structure["parameters"] = dict(email_msg.get_params() or [])
    structure["size"] = len(str(email_msg))

    if email_msg.is_multipart():
        structure["parts"] = []
        for part in email_msg.get_payload():
            if hasattr(part, "get_content_type"):
                part_structure = extract_bodystructure_from_email(part)
                structure["parts"].append(part_structure)

    return structure

def extract_body_from_email(email_msg) -> str:
    """Extract text body from email.Message object"""
    if email_msg.is_multipart():
        for part in email_msg.walk():
            if part.get_content_type() == "text/plain":
                payload = part.get_payload(decode=True)
                if payload:
                    return payload.decode("utf-8", errors="ignore")
    else:
        if email_msg.get_content_type() == "text/plain":
            payload = email_msg.get_payload(decode=True)
            if payload:
                return payload.decode("utf-8", errors="ignore")

    return ""

def extract_best_text_from_message(raw_bytes):
    """
    Given a raw RFC822 message (bytes), extract and decode the best displayable part:
    - Prefer text/plain
    - Fallback to text/html (decoded, as text)
    Returns a unicode string (decoded text) or None if not found.
    """
    import email
    from email import policy

    if not raw_bytes:
        return None

    if isinstance(raw_bytes, str):
        raw_bytes = raw_bytes.encode("utf-8", errors="replace")

    msg = email.message_from_bytes(raw_bytes, policy=policy.default)
    
    body = msg.get_body(preferencelist=("plain",))
    if body:
        text = body.get_content().strip()
        if text:
            return text
    
    body = msg.get_body(preferencelist=("html",))
    if body:
        html = body.get_content().strip()
        if html:
            return html
    return None

def extract_html_and_text_from_message(raw_bytes):
    """
    Given a raw RFC822 message (bytes), extract both HTML and plain text content.
    Returns a tuple of (html_content, text_content) where either can be None.
    """
    import email
    from email import policy

    if not raw_bytes:
        return None, None

    if isinstance(raw_bytes, str):
        raw_bytes = raw_bytes.encode("utf-8", errors="replace")

    msg = email.message_from_bytes(raw_bytes, policy=policy.default)
    
    html_content = None
    text_content = None
    
    
    html_body = msg.get_body(preferencelist=("html",))
    if html_body:
        html_content = html_body.get_content().strip()
        if not html_content:
            html_content = None
    
    
    text_body = msg.get_body(preferencelist=("plain",))
    if text_body:
        text_content = text_body.get_content().strip()
        if not text_content:
            text_content = None
    
    return html_content, text_content

def extract_html_from_email(email_msg) -> str:
    """Extract HTML body from email.Message object"""
    if email_msg.is_multipart():
        for part in email_msg.walk():
            if part.get_content_type() == "text/html":
                payload = part.get_payload(decode=True)
                if payload:
                    return payload.decode("utf-8", errors="ignore")
    else:
        if email_msg.get_content_type() == "text/html":
            payload = email_msg.get_payload(decode=True)
            if payload:
                return payload.decode("utf-8", errors="ignore")

    return ""

def detect_and_process_embedded_replies(content: str, is_html: bool = False) -> str:
    """
    Detect embedded replies in email content and wrap them in collapsible sections.
    
    Args:
        content: The email content (HTML or plain text)
        is_html: Whether the content is HTML or plain text
        
    Returns:
        Processed content with embedded replies wrapped in collapsible sections
    """
    if not content or not content.strip():
        return content
    
    if is_html:
        return _process_html_embedded_replies(content)
    else:
        return _process_plain_text_embedded_replies(content)

def _process_plain_text_embedded_replies(content: str) -> str:
    """Process embedded replies in plain text content"""
    lines = content.split('\n')
    processed_parts = []
    in_reply = False
    reply_buffer = []
    text_buffer = []
    
    for line in lines:
        is_reply_line = _is_embedded_reply_line(line)
        
        if is_reply_line and not in_reply:
            if text_buffer:
                text_content = '\n'.join(text_buffer)
                processed_parts.append(f'<pre>{_escape_html(text_content)}</pre>')
                text_buffer = []
            
            in_reply = True
            reply_buffer = [line]
        elif is_reply_line and in_reply:
            # Continue collecting reply lines
            reply_buffer.append(line)
        elif not is_reply_line and in_reply:
            if reply_buffer:
                reply_content = '\n'.join(reply_buffer)
                processed_parts.append(_create_collapsible_reply_section(reply_content))
                reply_buffer = []
            in_reply = False
            text_buffer.append(line)
        else:
            text_buffer.append(line)
    
    if in_reply and reply_buffer:
        reply_content = '\n'.join(reply_buffer)
        processed_parts.append(_create_collapsible_reply_section(reply_content))
    elif text_buffer:
        text_content = '\n'.join(text_buffer)
        processed_parts.append(f'<pre>{_escape_html(text_content)}</pre>')
    
    return ''.join(processed_parts)

def _process_html_embedded_replies(content: str) -> str:
    """Process embedded replies in HTML content"""
    content = re.sub(
        r'<blockquote[^>]*>(.*?)</blockquote>',
        lambda m: _create_collapsible_html_reply_section(m.group(1)),
        content,
        flags=re.DOTALL | re.IGNORECASE
    )
    
    lines = content.split('\n')
    processed_lines = []
    in_reply = False
    reply_buffer = []
    
    for line in lines:
        if re.search(r'^\s*(&gt;|>)', line.strip()):
            if not in_reply:
                in_reply = True
                reply_buffer = [line]
            else:
                reply_buffer.append(line)
        elif in_reply:
            if reply_buffer:
                reply_content = '\n'.join(reply_buffer)
                processed_lines.append(_create_collapsible_html_reply_section(reply_content))
                reply_buffer = []
            in_reply = False
            processed_lines.append(line)
        else:
            processed_lines.append(line)
    
    
    if in_reply and reply_buffer:
        reply_content = '\n'.join(reply_buffer)
        processed_lines.append(_create_collapsible_html_reply_section(reply_content))
    
    return '\n'.join(processed_lines)

def _escape_html(text: str) -> str:
    """Escape HTML special characters"""
    return (text.replace('&', '&amp;')
               .replace('<', '&lt;')
               .replace('>', '&gt;')
               .replace('"', '&quot;')
               .replace("'", '&#x27;'))

def _is_embedded_reply_line(line: str) -> bool:
    """Check if a line is part of an embedded reply"""
    line = line.strip()
    
            
    if not line:
        return False
    
            
    if line.startswith('>'):
        return True
    
    reply_patterns = [
        r'^On\s+.+wrote:?\s*$',  
        r'^-----Original Message-----',
        r'^From:\s+.+',
        r'^Sent:\s+.+',
        r'^To:\s+.+',
        r'^Subject:\s+.+',
                    r'^\d{1,2}/\d{1,2}/\d{2,4}.+wrote:?\s*$',  
            r'^Le\s+.+a\s+écrit\s*:?\s*$',  
            r'^Am\s+.+schrieb\s+.+:?\s*$',  
            r'^El\s+.+escribió\s*:?\s*$',  
    ]
    
    for pattern in reply_patterns:
        if re.match(pattern, line, re.IGNORECASE):
            return True
    
    return False

def _create_collapsible_reply_section(reply_content: str) -> str:
    """Create a collapsible section for plain text reply content"""
            
    lines = reply_content.strip().split('\n')
    summary = lines[0] if lines else "Previous message"
    
    
    summary = re.sub(r'^>+\s*', '', summary)
    if not summary.strip():
        summary = "Previous message"
    
    # Truncate long summaries
    if len(summary) > 80:
        summary = summary[:77] + "..."
    
    escaped_summary = _escape_html(summary)
    escaped_content = _escape_html(reply_content)
    
    return f"""<div class="embedded-reply-container">
    <div class="embedded-reply-toggle" onclick="toggleEmbeddedReply(this)">
        <span class="toggle-icon">▶</span>
        <span class="reply-summary">{escaped_summary}</span>
    </div>
    <div class="embedded-reply-content" style="display: none;">
        <pre>{escaped_content}</pre>
    </div>
</div>"""

def _create_collapsible_html_reply_section(reply_content: str) -> str:
    """Create a collapsible section for HTML reply content"""
    
    text_content = re.sub(r'<[^>]+>', ' ', reply_content)
    text_content = re.sub(r'\s+', ' ', text_content).strip()
    
    
    lines = text_content.split('\n')
    summary = lines[0] if lines else "Previous message"
    
    
    summary = re.sub(r'^>+\s*', '', summary)
    if not summary.strip():
        summary = "Previous message"
    
    # Truncate long summaries
    if len(summary) > 80:
        summary = summary[:77] + "..."
    
    escaped_summary = _escape_html(summary)
    
    return f"""<div class="embedded-reply-container">
    <div class="embedded-reply-toggle" onclick="toggleEmbeddedReply(this)">
        <span class="toggle-icon">▶</span>
        <span class="reply-summary">{escaped_summary}</span>
    </div>
    <div class="embedded-reply-content" style="display: none;">
        {reply_content}
    </div>
</div>"""
