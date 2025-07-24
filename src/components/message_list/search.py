import logging

class MessageSearch:
    def __init__(self):
        self.search_text = ""
        self.filtered_messages = []

    def set_search_text(self, search_text):
        logging.debug(f"MessageSearch: Search text changed to '{search_text}'")
        self.search_text = search_text.lower().strip()

    def apply_filter(self, messages):
        if not self.search_text:
            self.filtered_messages = messages.copy()
            return self.filtered_messages, True
        else:
            self.filtered_messages = []
            for message in messages:
                if self._message_matches_search(message, self.search_text):
                    self.filtered_messages.append(message)
            
            logging.debug(f"MessageSearch: Filtered {len(messages)} messages to {len(self.filtered_messages)} results")
            return self.filtered_messages, False

    def _message_matches_search(self, message, search_text):
        sender_name = message.get('sender', {}).get('name', '').lower()
        if search_text in sender_name:
            return True
            
        sender_email = message.get('sender', {}).get('email', '').lower()
        if search_text in sender_email:
            return True
            
        subject = message.get('subject', '').lower()
        if search_text in subject:
            return True
            
        return False

    def get_filtered_messages(self):
        return self.filtered_messages

    def has_search_text(self):
        return bool(self.search_text) 