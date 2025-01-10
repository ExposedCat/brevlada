import { fetchAccounts } from "./services/accounts.js";
import { Receiver } from "./services/receiver.js";
import { createSender } from "./services/sender.js";

const accounts = await fetchAccounts();
for (const account of accounts.slice(0, 1)) {
	console.log("Connecting to", account.receiver.host);
	const receiver = new Receiver(account);
	await receiver.connect();
	console.log("Account", account.receiver.username, "connected (Receive)");
	await receiver.initFolders();
	for (const folder of receiver.folders) {
		if (folder.kind === "folder") {
			console.log(folder.name, "...");
			continue;
		}
		for await (const message of receiver.fetchMailbox(folder.path)) {
			console.log(folder.name, message);
		}
	}
	receiver.dispose();
	console.log("Connecting to", account.sender.host);
	const sender = createSender(account);
	await sender.verify();
	console.log("Account", account.receiver.username, "connected (Send)");
	sender.close();
}
