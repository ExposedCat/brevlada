import { ImapFlow } from "imapflow";
import type { Account } from "./accounts.js";

export function createReceiver(account: Account) {
	const receiver = new ImapFlow({
		host: account.receiver.host,
		port: account.receiver.secure ? 993 : 143,
		secure: account.receiver.secure,
		auth: {
			user: account.receiver.username,
			accessToken: account.authToken,
		},
		emitLogs: false,
		logger: false,
	});

	receiver.on("error", console.error);

	receiver.once("close", () => {
		console.warn("Connection closed");
	});

	return {
		connect: () => receiver.connect(),
		disconnect: () => receiver.close(),
	};
}
