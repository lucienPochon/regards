import { AddAccount } from "./AddAccount";

/** The first screen, before there is an account: adding one, until its "Open Inbox". */
export function Welcome({ onDone }: { onDone: () => void }) {
  return (
    <main className="welcome">
      <AddAccount first onDone={onDone} />
    </main>
  );
}
