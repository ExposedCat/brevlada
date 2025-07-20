import email
import email.utils
import email.header
from typing import Dict, List, Optional, Any, Tuple
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
    # Try text/plain first
    body = msg.get_body(preferencelist=("plain",))
    if body:
        text = body.get_content().strip()
        if text:
            return text
    # Fallback to text/html (decoded, as text)
    body = msg.get_body(preferencelist=("html",))
    if body:
        html = body.get_content().strip()
        if html:
            return html
    return None
