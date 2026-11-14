;;; aevia-mode.el --- Major mode and LSP integration for Aevia  -*- lexical-binding: t; -*-

;; Copyright (C) 2026 The Aevia authors
;; Version: 0.1.0
;; Package-Requires: ((emacs "28.1"))
;; Keywords: languages
;; URL: https://github.com/Apich-Organization/aevia
;; SPDX-License-Identifier: Apache-2.0

;;; Commentary:

;; Syntax highlighting, comment handling, offside-rule indentation and
;; ready-made registration of the `aevia-lsp' server with both Eglot
;; (built in since Emacs 29) and lsp-mode.
;;
;; Installation:
;;
;;   (add-to-list 'load-path "/path/to/aevia/editors/emacs")
;;   (require 'aevia-mode)
;;
;; Eglot starts with M-x eglot, or automatically with
;;   (add-hook 'aevia-mode-hook #'eglot-ensure)
;; lsp-mode with M-x lsp, or
;;   (add-hook 'aevia-mode-hook #'lsp-deferred)
;;
;; The server is `aevia-lsp --stdio'; `aevia lsp' is the fallback when only
;; the CLI is on `exec-path'.  Set `aevia-lsp-server-command' to override.

;;; Code:

(require 'rx)

(defvar eglot-server-programs)
(defvar lsp-language-id-configuration)
(declare-function lsp-register-client "lsp-mode")
(declare-function make-lsp-client "lsp-mode")
(declare-function lsp-stdio-connection "lsp-mode")
(declare-function lsp-activate-on "lsp-mode")

(defgroup aevia nil
  "Aevia language support."
  :group 'languages
  :prefix "aevia-")

(defcustom aevia-indent-offset 4
  "Indentation step of the offside rule."
  :type 'integer
  :safe #'integerp)

(defcustom aevia-lsp-server-command nil
  "Command (list of strings) that starts the Aevia language server.
When nil, use `aevia-lsp --stdio' if it is on `exec-path', else
`aevia lsp --stdio'."
  :type '(choice (const :tag "Auto-detect" nil) (repeat string)))

(defun aevia-lsp-server-command ()
  "The command list used to start the language server."
  (or aevia-lsp-server-command
      (cond ((executable-find "aevia-lsp") '("aevia-lsp" "--stdio"))
            ((executable-find "aevia") '("aevia" "lsp" "--stdio"))
            (t '("aevia-lsp" "--stdio")))))

;;;; Syntax

(defvar aevia-mode-syntax-table
  (let ((table (make-syntax-table)))
    ;; `#' starts a line comment; `#-' ... `-#' is handled in
    ;; `aevia-syntax-propertize' because it nests.
    (modify-syntax-entry ?#  "<" table)
    (modify-syntax-entry ?\n ">" table)
    (modify-syntax-entry ?\" "\"" table)
    (modify-syntax-entry ?\\ "\\" table)
    (modify-syntax-entry ?_  "_" table)
    (modify-syntax-entry ?'  "_" table)
    (dolist (c '(?+ ?- ?* ?/ ?< ?> ?= ?! ?& ?| ?^ ?@ ?? ?:))
      (modify-syntax-entry c "." table))
    (modify-syntax-entry ?\( "()" table)
    (modify-syntax-entry ?\) ")(" table)
    (modify-syntax-entry ?\[ "(]" table)
    (modify-syntax-entry ?\] ")[" table)
    (modify-syntax-entry ?\{ "(}" table)
    (modify-syntax-entry ?\} "){" table)
    table))

(defun aevia--skip-block-comment ()
  "Point is just after `#-'.  Move past the matching `-#' (nesting).
Return the position of the final `#', or nil if the comment is unclosed."
  (let ((depth 1) (result nil))
    (while (and (> depth 0)
                (re-search-forward "#-\\|-#" nil t))
      (if (string= (match-string 0) "#-")
          (setq depth (1+ depth))
        (setq depth (1- depth))
        (when (= depth 0) (setq result (1- (point))))))
    result))

(defun aevia-syntax-propertize (start end)
  "Mark nestable block comments between START and END.
The opening and closing `#' become generic comment fences."
  (goto-char start)
  (while (re-search-forward "#-" end t)
    (let ((open (match-beginning 0)))
      ;; `#-' inside a line comment or a string is not a block comment.
      (unless (save-excursion
                (goto-char open)
                (let ((ppss (syntax-ppss)))
                  (or (nth 3 ppss) (nth 4 ppss))))
        (let ((close (aevia--skip-block-comment)))
          (put-text-property open (1+ open) 'syntax-table (string-to-syntax "!"))
          (if close
              (put-text-property close (1+ close) 'syntax-table (string-to-syntax "!"))
            (goto-char (point-max))))))))

;;;; Font lock

(defconst aevia-keywords
  '("as" "class" "derive" "dimension" "do" "else" "fn" "for" "if" "import" "in"
    "instance" "let" "match" "module" "open" "private" "rec" "test" "then"
    "type" "unit" "where" "with")
  "Reserved words (spec 1.5.2).")

(defconst aevia--ident "[[:alpha:]_][[:alnum:]_'₀-₉]*")
(defconst aevia--uident "[A-Z][[:alnum:]_'₀-₉]*")

(defconst aevia--number
  (concat "\\(?:0[xX][0-9a-fA-F_]+\\|0[oO][0-7_]+\\|0[bB][01_]+"
          "\\|[0-9][0-9_]*\\(?:\\.[0-9][0-9_]*\\)?\\(?:[eE][-+]?[0-9]+\\)?\\)"))

(defconst aevia--unit-atom
  "\\(?:[[:alpha:]°µμΩÅ][[:alnum:]_°µμΩÅ.]*\\|%\\)\\(?:\\^-?[0-9]+\\|\\^([-0-9/]+)\\|[⁰¹²³⁴⁵⁶⁷⁸⁹⁻⁺]+\\)?")

(defconst aevia-font-lock-keywords
  `(;; doc comments are comments by syntax; give them the doc face.
    ("^[ \t]*\\(##.*\\)$" 1 font-lock-doc-face prepend)
    ;; declarations
    (,(concat "\\_<\\(fn\\)\\_>[ \t]+\\(?:\\(rec\\)[ \t]+\\)?\\(" aevia--ident "\\)")
     (1 font-lock-keyword-face) (2 font-lock-keyword-face nil t)
     (3 font-lock-function-name-face))
    (,(concat "\\_<\\(let\\)\\_>[ \t]+\\(?:\\(rec\\)[ \t]+\\)?\\(" aevia--ident "\\)[ \t]*(")
     (3 font-lock-function-name-face))
    (,(concat "\\_<\\(type\\|class\\|instance\\|dimension\\)\\_>[ \t]+\\(" aevia--uident "\\)")
     (2 font-lock-type-face))
    (,(concat "\\_<\\(unit\\)\\_>[ \t]+\\(" aevia--ident "\\)")
     (2 font-lock-constant-face))
    (,(concat "\\_<\\(module\\|import\\|open\\)\\_>[ \t]+\\(" aevia--uident "\\(?:\\." aevia--uident "\\)*\\)")
     (2 font-lock-constant-face))
    ;; attributes
    (,(concat "@" aevia--ident) 0 font-lock-preprocessor-face)
    ;; quantity: number immediately followed by a unit expression (spec 1.8.1)
    (,(concat "\\(?:\\`\\|[^[:alnum:]_.]\\)\\(" aevia--number "\\)[ \t]*"
              "\\(" aevia--unit-atom "\\(?:[*·/]" aevia--unit-atom "\\)*\\)")
     (1 font-lock-constant-face) (2 font-lock-builtin-face))
    ;; plain numbers
    (,(concat "\\(?:\\`\\|[^[:alnum:]_]\\)\\(" aevia--number "\\)") 1 font-lock-constant-face)
    ;; keywords, booleans
    (,(regexp-opt aevia-keywords 'symbols) . font-lock-keyword-face)
    (,(regexp-opt '("true" "false" "oo" "nan") 'symbols) . font-lock-constant-face)
    ;; types and constructors
    (,(concat "\\_<" aevia--uident) . font-lock-type-face)
    ;; calls
    (,(concat "\\(" aevia--ident "\\)(") 1 font-lock-function-name-face)
    ;; string interpolation
    ("{[^{}\n\"]*}" 0 font-lock-variable-name-face prepend)))

;;;; Indentation (offside rule)

(defun aevia--opens-block-p ()
  "Non-nil if the current line ends with a block-introducing token."
  (save-excursion
    (end-of-line)
    (let ((limit (line-beginning-position)))
      ;; strip a trailing line comment
      (when (and (re-search-backward "[ \t]#\\([^-]\\|$\\)" limit t)
                 (not (nth 3 (syntax-ppss))))
        (skip-chars-backward " \t"))
      (skip-chars-backward " \t")
      (and (looking-back (rx (or "=" "->" "=>" "<-" "do" "then" "else" "with" "where"))
                         (max limit (- (point) 6)))
           t))))

(defun aevia-indent-line ()
  "Indent the current line: cycle through the plausible offside columns."
  (interactive)
  (let* ((cur (current-indentation))
         (at-text (<= (current-column) cur))
         (prev (save-excursion
                 (forward-line -1)
                 (while (and (not (bobp)) (looking-at "[ \t]*\\(#.*\\)?$"))
                   (forward-line -1))
                 (cons (current-indentation) (aevia--opens-block-p))))
         (top (if (cdr prev) (+ (car prev) aevia-indent-offset) (car prev)))
         (candidates (let (acc (c top))
                       (while (>= c 0) (push c acc) (setq c (- c aevia-indent-offset)))
                       (nreverse acc)))
         (next (if (and (eq last-command this-command) (memq cur candidates))
                   (or (cadr (memq cur candidates)) top)
                 top)))
    (indent-line-to next)
    (when (and at-text (< (current-column) next)) (back-to-indentation))))

;;;; Mode

;;;###autoload
(define-derived-mode aevia-mode prog-mode "Aevia"
  "Major mode for editing Aevia source."
  :syntax-table aevia-mode-syntax-table
  (setq-local comment-start "# ")
  (setq-local comment-start-skip "#+[ \t]*")
  (setq-local comment-end "")
  (setq-local indent-tabs-mode nil)
  (setq-local tab-width aevia-indent-offset)
  (setq-local indent-line-function #'aevia-indent-line)
  (setq-local syntax-propertize-function #'aevia-syntax-propertize)
  (setq-local font-lock-defaults '(aevia-font-lock-keywords nil nil nil))
  (setq-local font-lock-multiline t)
  (setq-local electric-indent-inhibit t))

;;;###autoload
(add-to-list 'auto-mode-alist '("\\.aev\\'" . aevia-mode))

;; Source files are always UTF-8 (spec 1.1); without this Emacs on Windows
;; guesses the locale code page and shows `°C', `µm' as mojibake.
;;;###autoload
(add-to-list 'auto-coding-alist '("\\.aev\\'" . utf-8))

;;;; LSP: Eglot and lsp-mode

(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs
               `(aevia-mode . ,(lambda (&optional _interactive _project)
                                 (aevia-lsp-server-command)))))

(with-eval-after-load 'lsp-mode
  (add-to-list 'lsp-language-id-configuration '(aevia-mode . "aevia"))
  (lsp-register-client
   (make-lsp-client
    :new-connection (lsp-stdio-connection #'aevia-lsp-server-command)
    :activation-fn (lsp-activate-on "aevia")
    :server-id 'aevia-lsp)))

(provide 'aevia-mode)
;;; aevia-mode.el ends here
