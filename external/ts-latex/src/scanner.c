#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <tree_sitter/parser.h>

enum TokenType {
  TRIVIA_RAW_FI,
  TRIVIA_RAW_ELSE,
  TRIVIA_RAW_ENV_COMMENT,
  TRIVIA_RAW_ENV_VERBATIM,
  TRIVIA_RAW_ENV_LISTING,
  TRIVIA_RAW_ENV_MINTED,
  TRIVIA_RAW_ENV_ASY,
  TRIVIA_RAW_ENV_ASYDEF,
  TRIVIA_RAW_ENV_PYCODE,
  TRIVIA_RAW_ENV_LUACODE,
  TRIVIA_RAW_ENV_LUACODE_STAR,
  TRIVIA_RAW_ENV_SAGESILENT,
  TRIVIA_RAW_ENV_SAGEBLOCK,
};

static bool find_verbatim(TSLexer *lexer, const char *keyword,
                          bool is_command_name) {
  bool has_marked = false;
  while (true) {
    if (lexer->eof(lexer)) {
      break;
    }

    bool advanced = false;
    bool failed = false;
    for (size_t i = 0; keyword[i]; i++) {
      if (lexer->eof(lexer)) {
        return has_marked;
      }

      if (lexer->lookahead != keyword[i]) {
        failed = true;
        break;
      }

      lexer->advance(lexer, false);
      advanced = true;
    }

    if (failed && !advanced) {
      lexer->advance(lexer, false);
      lexer->mark_end(lexer);
      has_marked = true;
      continue;
    }

    if (!failed) {
      if (is_command_name) {
        if (lexer->eof(lexer)) {
          return has_marked;
        }

        char c = lexer->lookahead;
        switch (c) {
        case ':':
        case '_':
        case '@':
          failed = true;
          break;
        default:
          failed = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');
          break;
        }

        if (failed) {
          lexer->mark_end(lexer);
          has_marked = true;
          continue;
        }
      }

      return has_marked;
    }
  }

  return has_marked;
}

static bool is_letter(int32_t c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '@';
}

// Skips the branch of a conditional like TeX does: up to the `\fi` (or with `stop_at_else`
// the `\else`) that closes it, not one in a comment, in a longer command name (`\fill`) or of
// a nested conditional. The token ends in front of the `\` of that `\fi` or `\else`.
static bool scan_conditional_branch(TSLexer *lexer, bool stop_at_else) {
  unsigned depth = 0;

  while (!lexer->eof(lexer)) {
    lexer->mark_end(lexer);

    if (lexer->lookahead == '%') {
      // A comment, up to the end of the line
      while (!lexer->eof(lexer) && lexer->lookahead != '\n') {
        lexer->advance(lexer, false);
      }
      continue;
    }

    if (lexer->lookahead != '\\') {
      lexer->advance(lexer, false);
      continue;
    }

    lexer->advance(lexer, false);
    if (lexer->eof(lexer)) {
      break;
    }
    if (!is_letter(lexer->lookahead)) {
      // A control symbol, e.g. `\%` or `\\`
      lexer->advance(lexer, false);
      continue;
    }

    // A control word: only its first letters are needed
    char name[12];
    unsigned length = 0;
    while (!lexer->eof(lexer) && is_letter(lexer->lookahead)) {
      if (length < sizeof(name)) {
        name[length] = (char)lexer->lookahead;
      }
      length++;
      lexer->advance(lexer, false);
    }

#define NAME_IS(literal)                                                       \
  (length == sizeof(literal) - 1 && memcmp(name, literal, length) == 0)

    if (NAME_IS("fi")) {
      if (depth == 0) {
        return true;
      }
      depth--;
    } else if (NAME_IS("else")) {
      if (depth == 0 && stop_at_else) {
        return true;
      }
    } else if (length >= 2 && name[0] == 'i' && name[1] == 'f' && !NAME_IS("iff") &&
               !NAME_IS("ifthenelse")) {
      // A nested conditional, e.g. `\if`, `\ifx` or `\ifdraft`; `\iff` and `\ifthenelse` are not
      depth++;
    }
#undef NAME_IS
  }
  return false;
}

static bool scan_until_fi(TSLexer *lexer) {
  return scan_conditional_branch(lexer, false);
}

// scan until else but no further than fi
static bool scan_until_else(TSLexer *lexer) {
  return scan_conditional_branch(lexer, true);
}

void *tree_sitter_latex_external_scanner_create() { return NULL; }

void tree_sitter_latex_external_scanner_destroy(void *payload) {}

unsigned tree_sitter_latex_external_scanner_serialize(void *payload,
                                                      char *buffer) {
  return 0;
}

void tree_sitter_latex_external_scanner_deserialize(void *payload,
                                                    const char *buffer,
                                                    unsigned length) {}

bool tree_sitter_latex_external_scanner_scan(void *payload, TSLexer *lexer,
                                             const bool *valid_symbols) {
  bool found = false;
  TSSymbol type = 0xFFFF;
  for (int i = 0; i <= TRIVIA_RAW_ENV_SAGEBLOCK; i++) {
    if (valid_symbols[i]) {
      if (found) {
        return false;
      } else {
        found = true;
        type = i;
      }
    }
  }

  lexer->result_symbol = type;
  switch (type) {
  case TRIVIA_RAW_ELSE:
    /* return find_verbatim(lexer, "\\else", true); */
    return scan_until_else(lexer);
  case TRIVIA_RAW_FI:
    /* return find_verbatim(lexer, "\\fi", true); */
    return scan_until_fi(lexer);
  case TRIVIA_RAW_ENV_COMMENT:
    return find_verbatim(lexer, "\\end{comment}", false);
  case TRIVIA_RAW_ENV_VERBATIM:
    return find_verbatim(lexer, "\\end{verbatim}", false);
  case TRIVIA_RAW_ENV_LISTING:
    return find_verbatim(lexer, "\\end{lstlisting}", false);
  case TRIVIA_RAW_ENV_MINTED:
    return find_verbatim(lexer, "\\end{minted}", false);
  case TRIVIA_RAW_ENV_PYCODE:
    return find_verbatim(lexer, "\\end{pycode}", false);
  case TRIVIA_RAW_ENV_ASY:
    return find_verbatim(lexer, "\\end{asy}", false);
  case TRIVIA_RAW_ENV_ASYDEF:
    return find_verbatim(lexer, "\\end{asydef}", false);
  case TRIVIA_RAW_ENV_LUACODE:
    return find_verbatim(lexer, "\\end{luacode}", false);
  case TRIVIA_RAW_ENV_LUACODE_STAR:
    return find_verbatim(lexer, "\\end{luacode*}", false);
  case TRIVIA_RAW_ENV_SAGESILENT:
    return find_verbatim(lexer, "\\end{sagesilent}", false);
  case TRIVIA_RAW_ENV_SAGEBLOCK:
    return find_verbatim(lexer, "\\end{sageblock}", false);
  }

  return false;
}
