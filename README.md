# SPEAK WITH

A simple slack alternative in the works made in the TAMASHA stack.

Current features include :-
1. Public and private chat rooms
2. User to user private chats with an option to have multiple users per private chat.
3. user management - new users use the registration link, but are not activated unless the admin allows them.
4. Tiny things - like user profile images etc.

Future planned work :-
   1. File uploads through chat.
   2. Emojis in messages, and generally richer text messages with code blocks and user and channel tagging in the messages.
   3. Automatic backups to storage providers.
   4. User online indicators and unread message counts.
   5. Notifications!
   6. Archiving channels
   7. Automatic SSL
   8. Cross workspace connections (a la slack connections)
   9. Ui/ux overhaul? maybe. Live with programmer art for now.
   

## Build and run

Install the required build dependencies and dev dependencies :-

1. Tailwindcss - executable binary for running the tailwind compiler
2. Cargo - rust build tooling
3. just - easy make files that just work

In dev mode,

`just run`

will build and run the project. All data is stored in the `data` folder.

Run `cargo build` to build the project and then execute the `chat` binary that gets generated. 



