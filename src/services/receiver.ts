import { ImapFlow, type ListResponse } from "imapflow";
import type { Account } from "./accounts.js";

export type Folder = {
	name: string;
	kind:
		| "regular"
		| "folder"
		| "archinbox"
		| "inbox"
		| "sent"
		| "trash"
		| "drafts"
		| "junk"
		| "archive"
		| "all";
	path: string;
	children: Folder[];
	isNested: boolean;
};

const USE_KIND_MAP: Record<string, Folder["kind"]> = {
	"\\\\Inbox": "inbox",
	"\\\\Sent": "sent",
	"\\\\Trash": "trash",
	"\\\\Drafts": "drafts",
	"\\\\Junk": "junk",
	"\\\\Archive": "archive",
	"\\\\All": "all",
};

export class Receiver {
	private smtp: ImapFlow;

	folders: Folder[] = [];

	constructor(account: Account) {
		this.smtp = new ImapFlow({
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
	}

	async initFolders() {
		const folders = (await this.smtp.list()) as unknown as (ListResponse & {
			parentPath?: string;
		})[];
		const rootFolders = new Map<string, Folder>(
			folders.map((folder) => [
				folder.path,
				{
					name: folder.name,
					kind: USE_KIND_MAP[folder.specialUse] ?? "regular",
					path: folder.path,
					children: [],
					isNested: false,
				},
			]),
		);

		for (const folder of folders) {
			const entry = rootFolders.get(folder.path);
			if (entry && folder.parentPath) {
				entry.isNested = true;
				const parent = rootFolders.get(folder.parentPath);
				if (parent) {
					parent.kind = "folder";
					parent.children.push(entry);
				}
			}
		}

		this.folders = Array.from(rootFolders.values()).filter(
			(folder) => !folder.isNested,
		);
	}

	async *fetchMailbox(name: string) {
		const lock = await this.smtp.getMailboxLock(name);
		try {
			for await (const message of this.smtp.fetch("*", { envelope: true })) {
				yield message;
			}
		} catch (error) {
			lock.release();
			throw error;
		}
		lock.release();
	}

	async connect() {
		return this.smtp.connect();
	}

	dispose() {
		this.smtp.close();
	}

	get raw() {
		return this.smtp;
	}
}
