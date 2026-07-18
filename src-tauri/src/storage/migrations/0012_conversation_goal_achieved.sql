-- Whether the conversation's CURRENT goal has been confirmed met by the
-- observer (at FinishTask). Reset to 0 whenever the goal is set/changed/cleared;
-- set to 1 when the observer approves the goal. Lets the "Goal achieved" banner
-- survive a reload instead of reverting to "Pursuing goal".
ALTER TABLE conversations ADD COLUMN goal_achieved INTEGER NOT NULL DEFAULT 0;
