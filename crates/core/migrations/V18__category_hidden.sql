-- Hidden categories (ROADMAP M3.7, the categories editor; `docs/ARCHITECTURE.md`
-- → "Decision record: categories" and "Decision record: the categories editor
-- (built)").
--
-- A category that something uses can't be deleted (a transaction, split line,
-- budget, schedule or rule points at it), so the user hides it instead. A
-- hidden category drops out of the pickers and menus that offer categories,
-- but keeps its transactions, its history and its place in reports; showing
-- it again brings it back. Hiding a group hides its subcategories with it in
-- the pickers, while each keeps its own flag, so showing the group again
-- restores them as they were (`coffer_core::category::CategoryTree::visible`).

ALTER TABLE category ADD COLUMN hidden INTEGER NOT NULL DEFAULT 0
    CHECK (hidden IN (0, 1));
