import atexit
from app import MyApp
from utils.mail import cleanup_all_connections


if __name__ == "__main__":
    atexit.register(cleanup_all_connections)

    app = MyApp()

    def cleanup_on_exit():
        if hasattr(app, "window") and hasattr(app.window, "message_list"):
            app.window.message_list.cleanup()

    atexit.register(cleanup_on_exit)
    app.run(None)
