build:
	glib-compile-resources --target=resources.gresource --sourcedir=resources resources/resources.gresource.xml

start:
	python3 src/main.py

format:
	black src
