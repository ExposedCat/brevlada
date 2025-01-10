import { fetchAccounts } from "./services/accounts.js";
import { createReceiver } from "./services/receiver.js";
import { createSender } from "./services/sender.js";
const accounts = await fetchAccounts();
for (const account of accounts) {
    console.log("Connecting to", account.receiver.host);
    const receiver = createReceiver(account);
    await receiver.connect();
    console.log("Account", account.receiver.username, "connected (Receive)");
    receiver.disconnect();
    console.log("Connecting to", account.sender.host);
    const sender = createSender(account);
    await sender.verify();
    console.log("Account", account.receiver.username, "connected (Send)");
    sender.close();
}
//# sourceMappingURL=index.js.map