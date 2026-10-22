import { AccountForm } from "./AccountForm";
import { AppMark } from "./AppMark";

/** The first screen, before there is an account. */
export function Welcome() {
  return (
    <main className="welcome">
      <section className="welcome-card">
        <AppMark size={88} draw pulse />
        <h1>Welcome to Regards</h1>
        <p className="muted">
          Add your email account to begin. Regards keeps your password or sign-in in this computer's credential
          store, and your mail on this computer.
        </p>
        <AccountForm onDone={() => {}} />
      </section>
    </main>
  );
}
