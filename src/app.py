from utils.toolkit import Adw
from window import MyWindow


class MyApp(Adw.Application):
    def __init__(self):
        super().__init__(application_id="org.example.OnlineAccounts")
        self.connect("activate", self.do_activate)

    def do_activate(self):
        self.window = MyWindow(self)
        self.window.present() 