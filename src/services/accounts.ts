import dbus, { type Variant } from "dbus-next";

export type Account = {
	receiver: {
		host: string;
		secure: boolean;
		username: string;
	};
	sender: {
		host: string;
		secure: boolean;
		username: string;
	};
	authToken: string;
};

export async function fetchAccounts() {
	const bus = dbus.sessionBus();
	const goaObject = await bus.getProxyObject(
		"org.gnome.OnlineAccounts",
		"/org/gnome/OnlineAccounts",
	);
	const objectManager = goaObject.getInterface(
		"org.freedesktop.DBus.ObjectManager",
	);
	const managedObjects = await objectManager.GetManagedObjects();
	const entries = Object.entries(managedObjects) as [
		string,
		// biome-ignore lint/suspicious/noExplicitAny: <explanation>
		any,
	][];

	const accounts: Account[] = [];
	for (const [path, interfaces] of entries) {
		if (
			"org.gnome.OnlineAccounts.Mail" in interfaces &&
			"org.gnome.OnlineAccounts.OAuth2Based" in interfaces
		) {
			const account = await bus.getProxyObject(
				"org.gnome.OnlineAccounts",
				path,
			);
			const properties = account.getInterface(
				"org.freedesktop.DBus.Properties",
			);
			const oauth2 = account.getInterface(
				"org.gnome.OnlineAccounts.OAuth2Based",
			);
			const [authToken] = await oauth2.GetAccessToken();

			const $ = async (
				field: string,
				target = "org.gnome.OnlineAccounts.Mail",
			) => {
				const variant = (await properties.Get(target, field)) as Variant;
				return variant.value;
			};

			const receiverHost = await $("ImapHost");
			const receiverSecure = await $("ImapUseSsl");
			const receiverUsername = await $("ImapUserName");

			const senderHost = await $("SmtpHost");
			const senderSecure = await $("SmtpUseSsl");
			const senderUsername = await $("SmtpUserName");

			accounts.push({
				receiver: {
					host: receiverHost,
					secure: receiverSecure,
					username: receiverUsername,
				},
				sender: {
					host: senderHost,
					secure: senderSecure,
					username: senderUsername,
				},
				authToken,
			});
		}
	}

	bus.disconnect();
	return accounts;
}
