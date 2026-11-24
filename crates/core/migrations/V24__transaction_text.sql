-- A full-text index over a transaction's text, so the ledger's text filter
-- (`search_transactions`) reads an index instead of scanning every row
-- (ROADMAP FT.2; FT.1's decision record in `docs/architecture/reporting.md`).
--
-- Indexed: the display merchant, the bank's merchant and the raw description.
-- Notes are never indexed. Every row of "transaction" is indexed, whatever its
-- exclusion; the ledger decides what counts.
--
-- An external-content FTS5 table: the text stays in "transaction", and the
-- index holds only the trigrams and which rows have them, with the row ID as
-- the rowid. `trigram` answers "contains this text" for text of three or more
-- characters; `case_sensitive 0` folds case over all of Unicode (SQLite's
-- `LIKE` folds ASCII only), so the index returns a superset of what `LIKE`
-- matches and the ledger keeps `LIKE` as the final check. Diacritics are not
-- removed.
--
-- The triggers keep it in step. An external-content table is told what to
-- forget with the 'delete' command and the OLD values, so they must be the
-- values the index was built from. Only "transaction" has the text (the display
-- merchant is its column), so only it needs triggers. A rebuild of
-- "transaction" drops them: recreate them (`migrations/README.md`).

CREATE VIRTUAL TABLE transaction_text USING fts5 (
    merchant_display,
    merchant,
    description_raw,
    content = 'transaction',
    content_rowid = 'id',
    tokenize = 'trigram case_sensitive 0'
);

CREATE TRIGGER transaction_text_insert AFTER INSERT ON "transaction"
BEGIN
    INSERT INTO transaction_text (rowid, merchant_display, merchant, description_raw)
    VALUES (NEW.id, NEW.merchant_display, NEW.merchant, NEW.description_raw);
END;

CREATE TRIGGER transaction_text_delete AFTER DELETE ON "transaction"
BEGIN
    INSERT INTO transaction_text (transaction_text, rowid, merchant_display, merchant,
                                  description_raw)
    VALUES ('delete', OLD.id, OLD.merchant_display, OLD.merchant, OLD.description_raw);
END;

CREATE TRIGGER transaction_text_update
    AFTER UPDATE OF merchant_display, merchant, description_raw ON "transaction"
BEGIN
    INSERT INTO transaction_text (transaction_text, rowid, merchant_display, merchant,
                                  description_raw)
    VALUES ('delete', OLD.id, OLD.merchant_display, OLD.merchant, OLD.description_raw);
    INSERT INTO transaction_text (rowid, merchant_display, merchant, description_raw)
    VALUES (NEW.id, NEW.merchant_display, NEW.merchant, NEW.description_raw);
END;

-- The rows already there.
INSERT INTO transaction_text (transaction_text) VALUES ('rebuild');
