# TTRPG Scheduling

A small app that lets one group of friends schedule tabletop RPG sessions and keep each
campaign's knowledge (description, pages, session history) in one place.

## People and roles

**User**:
A person with an account in the app. Accounts are never deleted, only deactivated.
_Avoid_: Account, player (for someone outside a campaign)

**Site admin**:
A user who manages accounts and campaigns across the whole site. Being a site admin doesn't let
you run a campaign; to do that, an admin takes over the GM role.
_Avoid_: Admin (when it could be confused with the GM), superuser

**Member**:
A user who belongs to a campaign. The GM is a member too.
_Avoid_: Participant

**GM**:
The one member who runs a campaign. Each campaign has exactly one GM, and the role can be handed
to another member.
_Avoid_: DM, game master (in short UI text), owner, creator

**Player**:
A member of a campaign who isn't its GM.

**Deactivated user**:
A user who can no longer log in. Their content stays.
_Avoid_: Deleted user, banned user

## Campaigns

**Campaign**:
One ongoing game, played by a fixed group of members with one GM. Everything else (pages,
scheduling polls, game sessions, comments, polls) belongs to a campaign, and all of it is visible
to every member.
_Avoid_: Game, group, table

**System**:
The rule system a campaign is played with (e.g. "D&D 5e", "Blades in the Dark").
_Avoid_: Ruleset, game

**Campaign page**:
A GM-written page of campaign knowledge (setting, house rules, characters), in addition to the
campaign description.
_Avoid_: Wiki page, article, note

**Invite**:
A link the GM creates so that people can join a campaign. It's usable a set number of times
and expires after 14 days. Anyone without an account registers through an invite.
_Avoid_: Invitation code, sign-up link

**Dashboard**:
The home page: the user's campaigns, the scheduling polls waiting for their vote, and upcoming
game sessions.
_Avoid_: Home, overview (the campaign's own page is its overview)

## Scheduling

**Scheduling poll**:
The GM's question "when do we play next?", which offers several candidate dates. Members vote on
each date, then the GM decides. Always say the full name, so it isn't confused with a poll.
_Avoid_: Schedule, date poll, Doodle

**Candidate date**:
One date and start time offered in a scheduling poll.
_Avoid_: Option, slot, choice

**Answer**:
A member's vote on one candidate date: yes, maybe or no.
_Avoid_: Availability, response, RSVP

**Deadline**:
The optional time after which members can no longer vote in a scheduling poll. The GM can
still decide after it.
_Avoid_: Closing time, due date

**Decide**:
The GM picks one candidate date, which turns the scheduling poll into a game session. It
doesn't have to be the date with the most yes answers.
_Avoid_: Finalize, confirm, close

**Reopen**:
Undoing a decision: the scheduling poll is open again and the game session it created goes
away. This is only possible while that game session hasn't been touched yet.
_Avoid_: Undecide, revert

**Regular slot**:
A campaign's usual playing time (e.g. "Fri and Sat at 18:00, every 2 weeks"). It never schedules
anything by itself; it only fills in the candidate dates of a new scheduling poll.
_Avoid_: Pattern, recurrence, recurring session

## Game sessions

**Game session**:
One meeting of a campaign to play, planned or already played. Always say "game session" (or
"session" where only game sessions can be meant), never just "session" in a context that also
involves logins.
_Avoid_: Session (ambiguous), event, meeting, game night

**Session number**:
A game session's place in its campaign's history (1, 2, 3, …), always in date order. Adding a
game session for an earlier date shifts the numbers of the ones after it.
_Avoid_: Episode

**Planned / Played / Cancelled**:
The states of a game session. Game sessions are cancelled, never deleted.

**Attendance**:
The members who were at a game session. It starts as the members who answered yes (or all
members for a game session added directly), and the GM can correct it.
_Avoid_: Attendees list, RSVPs, participants

**Entry**:
A piece of writing about one game session, written by any member. It has a kind: recap,
notes, loot or other.
_Avoid_: Post, log, journal

**Recap**:
An entry that tells what happened in a game session.
_Avoid_: Summary, session report

**History**:
A campaign's list of game sessions in date order.
_Avoid_: Log, timeline, archive

## Discussion

**Comment**:
A member's Markdown message on a campaign, a game session or a scheduling poll.
_Avoid_: Post, reply, message

**Poll**:
A GM's question to the members with fixed choices (single or multiple choice), on a campaign or
a game session. It isn't a scheduling poll.
_Avoid_: Survey, vote (for the poll itself)

## Login

**Auth session**:
A user's login, which lasts up to 30 days. It has nothing to do with game sessions.
_Avoid_: Session (ambiguous), login session
