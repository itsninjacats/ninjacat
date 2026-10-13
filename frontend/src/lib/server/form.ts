export function formText(form: FormData, name: string): string {
	return form.get(name)?.toString().trim() ?? '';
}
