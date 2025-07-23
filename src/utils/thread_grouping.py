import re
from datetime import datetime
from typing import List, Optional, Union, Dict, Any
from models.message import Message
from models.thread import MessageThread

def group_messages_into_threads(
    messages: List[Union[Message, Dict[str, Any]]]
) -> List[MessageThread]:
    """Group messages into threads based on subject and references"""
    threads = []

    for message in messages:
        thread = find_thread_for_message(message, threads)
        if thread:
            thread.add_message(message)
        else:
            
            subject = get_thread_subject(message)
            thread = MessageThread(subject)
            thread.add_message(message)
            threads.append(thread)

    
    threads.sort(key=lambda t: t.latest_date or datetime.min, reverse=True)

    return threads

def find_thread_for_message(
    message: Union[Message, Dict[str, Any]], threads: List[MessageThread]
) -> Optional[MessageThread]:
    """Find the appropriate thread for a message"""
    
    thread = find_thread_by_references(message, threads)
    if thread:
        return thread

    
    normalized_subject = get_thread_subject(message)
    if normalized_subject:
        for thread in threads:
            if thread.subject.lower() == normalized_subject.lower():
                return thread

    return None

def find_thread_by_references(
    message: Union[Message, Dict[str, Any]], threads: List[MessageThread]
) -> Optional[MessageThread]:
    """Find thread using message references"""
    thread_references = get_message_attr(message, "thread_references", [])
    if not thread_references:
        return None

    message_id = get_message_attr(message, "message_id", "")

    
    for thread in threads:
        for thread_message in thread.messages:
            thread_msg_id = get_message_attr(thread_message, "message_id", "")
            thread_msg_refs = get_message_attr(thread_message, "thread_references", [])

            
            if thread_msg_id and thread_msg_id in thread_references:
                return thread
            
            if message_id and message_id in thread_msg_refs:
                return thread

    return None

def normalize_subject(subject: str) -> str:
    """Normalize subject line for threading"""
    if not subject:
        return ""

    
    normalized = re.sub(
        r"^(Re|Fwd?|AW|Antw|回复|转发):\s*", "", subject, flags=re.IGNORECASE
    )

    
    normalized = re.sub(r"\s+", " ", normalized).strip()

    
    normalized = re.sub(r"\s*\[.*?\]\s*$", "", normalized)  
    normalized = re.sub(r"\s*\(.*?\)\s*$", "", normalized)  

    return normalized

def parse_references(references_header: str) -> List[str]:
    """Parse References header into list of message IDs"""
    if not references_header:
        return []

    
    message_ids = re.findall(r"<([^>]+)>", references_header)

    
    seen = set()
    unique_ids = []
    for msg_id in message_ids:
        if msg_id not in seen:
            seen.add(msg_id)
            unique_ids.append(msg_id)

    return unique_ids

def get_message_attr(message: Union[Message, Dict[str, Any]], attr: str, default=None):
    """Get attribute from message (works with both Message objects and dicts)"""
    if isinstance(message, dict):
        return message.get(attr, default)
    else:
        return getattr(message, attr, default)

def get_thread_subject(message: Union[Message, Dict[str, Any]]) -> str:
    """Get normalized subject for threading"""
    if isinstance(message, dict):
        subject = message.get("subject", "") or message.get("thread_subject", "")
    else:
        subject = getattr(message, "get_thread_subject", lambda: message.subject)()

    return normalize_subject(subject)
