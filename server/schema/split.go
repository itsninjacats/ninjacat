package schema

import "strings"

// SplitStatements splits a migration file into individual statements.
//
// It exists because the ClickHouse Go driver executes exactly ONE statement
// per Exec — sending a whole file produces a syntax error at the first
// semicolon. This is not a SQL parser; it is a small scanner that is honest
// about the cases the real files contain:
//
//   - "--" line comments are stripped before splitting. The schema files are
//     full of them (many in Polish, with punctuation), and a semicolon inside
//     a comment must not split. "--" INSIDE a string literal is content, not
//     a comment, so string state is tracked in the same pass.
//   - "/* */" block comments are stripped too, replaced by one space so the
//     removal cannot glue two tokens together.
//   - ";" splits only OUTSIDE quotes. The schema is full of quoted strings —
//     tags['env'] map keys, Enum8('OK' = 0, ...) — and a semicolon inside
//     one must survive.
//   - Inside '...' both ClickHouse escapes are honored: backslash escapes the
//     next character, and a doubled single-quote reads as close-then-reopen,
//     which is equivalent for splitting purposes. Backtick and double-quote
//     identifiers are tracked the same way.
//
// Empty and whitespace-only fragments (a trailing semicolon, a comment-only
// stretch) are dropped rather than sent to the server.
func SplitStatements(sql string) []string {
	var out []string
	var b strings.Builder

	flush := func() {
		if stmt := strings.TrimSpace(b.String()); stmt != "" {
			out = append(out, stmt)
		}
		b.Reset()
	}

	i, n := 0, len(sql)
	for i < n {
		c := sql[i]
		switch {
		case c == '\'' || c == '`' || c == '"':
			// Quoted string or identifier: copy through verbatim, honoring
			// backslash escapes so an escaped quote cannot end it early.
			q := c
			b.WriteByte(c)
			i++
			for i < n {
				if sql[i] == '\\' && i+1 < n {
					b.WriteByte(sql[i])
					b.WriteByte(sql[i+1])
					i += 2
					continue
				}
				b.WriteByte(sql[i])
				if sql[i] == q {
					i++
					break
				}
				i++
			}
		case c == '-' && i+1 < n && sql[i+1] == '-':
			// Line comment: drop to end of line, keep the newline itself so
			// "a -- x\nb" stays two tokens.
			for i < n && sql[i] != '\n' {
				i++
			}
		case c == '/' && i+1 < n && sql[i+1] == '*':
			// Block comment: drop it, leave a space in its place.
			i += 2
			for i < n {
				if sql[i] == '*' && i+1 < n && sql[i+1] == '/' {
					i += 2
					break
				}
				i++
			}
			b.WriteByte(' ')
		case c == ';':
			flush()
			i++
		default:
			b.WriteByte(c)
			i++
		}
	}
	flush()
	return out
}
