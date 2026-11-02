-- Flag changes to newly arrived mail could get lost before 0.1.6. Forgetting the last seen
-- modification sequence makes the next sync read every flag of the loaded range once.
UPDATE folders SET highestmodseq = NULL;
