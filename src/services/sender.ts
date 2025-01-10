import { createTransport } from "nodemailer";
import type { Account } from "./accounts";

export function createSender(account: Account) {
	return createTransport({
		host: account.sender.host,
		port: account.sender.secure ? 465 : 587,
		secure: account.sender.secure,
		auth: {
			type: "OAUTH2",
			user: account.sender.username,
			accessToken: account.authToken,
		},
	});
}
