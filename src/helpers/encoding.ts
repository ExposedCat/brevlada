export function base64Encode(input: string): string {
	return Buffer.from(input).toString("base64");
}
