import { fetchAccounts } from "./services/accounts.js";
import { createReceiver } from "./services/receiver.js";

const accounts = await fetchAccounts();
for (const account of accounts) {
	console.log("Connecting to", account.receiver.host);
	const receiver = createReceiver(account);
	await receiver.connect();
	console.log("Account", account.receiver.username, "connected");
	receiver.disconnect();
}
