# Looper: "Done means done" (story film brief)

Built with the `business-motion-film` workflow from
[echris6/motion-video-kit](https://github.com/echris6/motion-video-kit). The story structure uses
Blake Snyder's *Save the Cat!* beat sheet and Robert McKee's *Story*.

## Brief

| Item | Value |
| --- | --- |
| Business | Looper, a local macOS control plane with an iPhone companion that keeps AI coding-agent sessions moving when they stop |
| Buyer | Developers who run long Codex / Claude Code / Cursor sessions and babysit them |
| Viewer's problem | The agent says "done" when tests still fail, and you send "keep going" again and again |
| Single action (CTA) | Visit **looper.fyi** |
| Provably true | The modes are Infinite, Await Reply, Completion Checks and Max Turns 1/2/3. Completion Checks runs your commands and keeps the agent going until they pass. The iPhone push title is "Session stopped". You can reply from your phone into the same session. It runs locally. It supports Codex, Claude Code, Cursor, Zed, Devin, Grok Build and OpenCode, among others. |
| Never claim | Speed-ups, customer counts, testimonials or cloud hosting. The film never says the agent is always right. |
| Assets | Looper orb logo (`assets/looper-logo.png`) and site tokens. There is no AI footage: every frame is HTML, CSS and SVG. |
| Format | 1920×1080, 60 fps, 48 s, with a music bed and sparse UI effects |
| Brand | Paper `oklch(12.5% .009 268)`, ink `oklch(95% .006 270)`, lavender accent `oklch(76% .14 292)`, SF Pro Display/Text, SF Mono for the terminal. The film moves from night to a pearl dawn. |
| Label | End card: "Dramatization · UI simplified" |

## Story design

**McKee controlling idea (value + cause):** *The work finishes, and you get your night back, when
"done" is decided by your checks instead of the agent's word.*

**Protagonist:** you, the developer. You're never shown, only implied by your messages, your clock and
your absence. **Conscious desire:** ship the auth refactor tonight. **Unconscious need:** to stop
babysitting and trust a loop.

**Antagonism:** the premature "done". It escalates from a false claim to failing checks, and finally to a
decision only a human can make, which arrives while you're away.

**Gap:** at 9.5 s the expectation "All done!" meets the result "14 failed".

**Crisis / true dilemma (McKee):** remove the legacy endpoint and break three clients, or keep the
debt. The agent can't choose; you must, from wherever you are.

**Save the Cat moment:** at the height of the grind, the developer still types "please" to the
agent (the first "keep going, please"), so we like them before they suffer.

**Motifs:**
- The **loop ring** is the persistent actor. It's born from the "Session stopped" pill, breaks at All
  Is Lost, and closes at the Finale.
- The **clock chip** tracks the passing night, from 11:47 PM to 7:02 AM.
- The **same terminal session** frames both the Opening and Final images.

## Storyboard v2 (50 s at 60 fps, after the storyboard critic)

The critic's fixes applied here:
- **All Is Lost is not Looper failing.** Once the checks pass, Looper correctly lets the session stop. The loss is that the agent stopped on a question at 3:40 AM while you're away.
- **Every stop re-runs the checks.** The mode is set per session and is evaluated at every stop, so the Finale's re-check is honest.
- **The crisis is a true dilemma with a cost:** "Cleaner code, but 3 client apps break." The reply picks a side: "Keep it. Clients first."
- **No speed-up claims.** Laps have a constant length.
- **Layouts vary:** a bubble wall, a loop ring, check tiles, a phone lock screen and a Mac/phone split.
- **Labelling:** "Dramatization · UI simplified" is on screen for the whole film, and **looper.fyi** is the largest end-card element, held for 5.6 s.

| # | Time | Beat | Picture | Value |
|---|---|---|---|---|
| 1 | 0.0–3.2 | Opening Image | The Codex terminal at 11:47 PM. "Refactor auth. Run pnpm test before you finish." | Hope + |
| 2 | 3.2–5.0 | Theme Stated | A continuous push into "before you finish". **Done should mean done.** | + |
| 3 | 5.0–9.6 | Set-up + Save the Cat | A wall of "keep going" bubbles, starting with "keep going, please". Counter ×23, clock to 1:58 | − |
| 4 | 9.6–12.4 | Catalyst (gap) | The wall collapses into one line: "✓ All done!", then **Session stopped**, then `pnpm test` ✗ 14. Hard cut to a giant **14 failed** | −− |
| 5 | 12.4–13.8 | Debate | A close-up of the input: "keep go" is typed and then deleted. **Again?** | − |
| 6 | 13.8–16.6 | Break into Two | The orb pulls the Session stopped pill, which bends into the **loop ring**. The picker lands on **Completion Checks** | + |
| 7 | 16.6–23.6 | Fun & Games + B story | The orb laps the ring past four stations; failing tests fall 14 → 9 → 3 → 0. "2:21 AM · you went to bed", and a progress line reaches the phone | + |
| 8 | 23.6–26.4 | Midpoint | Three check tiles. The test tile goes green (false victory), while lint ✗ 2 and typecheck ✗ 1 are sent back, then ✓ ✓ ✓ | + / stakes |
| 9 | 26.4–28.4 | Bad Guys Close In | The agent asks: "Remove /v1/login? Cleaner code, but 3 client apps break." | − |
| 10 | 28.4–30.4 | All Is Lost | The checks pass, so the session stops (correctly). The ring dims to a halt at 3:40 AM. **Stopped. With a question.** | −− |
| 11 | 30.4–31.6 | Dark Night | Near black, with a waiting caret. **It needs you.** | − |
| 12 | 31.6–35.6 | Break into Three | Lock screen: **Session stopped · MacBook Pro**, with the question. Reply: "Keep it. Clients first. Add a deprecation warning." | + |
| 13 | 35.6–41.2 | Finale | The reply flies into the **same session**. The ring relights; the agent works and stops; the checks re-run ✓ ✓ ✓ | ++ |
| 14 | 41.2–44.4 | Final Image | This mirrors the Opening in the same framing. The clock rolls 3:52 → 7:02 AM and the palette moves from night to dawn | ++ |
| 15 | 44.4–50.0 | End card | The orb and **looper.fyi** (largest element), with the line "Keeps your coding agents going until your checks pass." | ++ |
