export interface EmailVerificationMessage {
  user_id: string;
  email: string;
  display_name: string;
  verification_url: string;
  expires_at: string;
}

export interface PasswordResetMessage {
  user_id: string;
  email: string;
  display_name: string;
  reset_url: string;
  expires_at: string;
}

export interface EmailVerificationSender {
  sendVerification(message: EmailVerificationMessage): Promise<void>;
  sendPasswordReset?(message: PasswordResetMessage): Promise<void>;
}

export class HttpEmailVerificationSender implements EmailVerificationSender {
  readonly #endpoint: string;
  readonly #authorization: string;

  constructor(endpoint: string, authorizationToken: string) {
    const url = new URL(endpoint);
    if (
      url.protocol !== "https:" ||
      url.username !== "" ||
      url.password !== "" ||
      url.hash !== ""
    ) {
      throw new Error("email delivery endpoint must be an HTTPS URL");
    }
    if (
      authorizationToken.length < 32 ||
      authorizationToken.length > 512 ||
      /[\u0000-\u001f\u007f]/u.test(authorizationToken)
    ) {
      throw new Error("email delivery token is invalid");
    }
    this.#endpoint = url.toString();
    this.#authorization = `Bearer ${authorizationToken}`;
  }

  async sendVerification(message: EmailVerificationMessage): Promise<void> {
    await this.#send("agent-pulse-email-verification", message.email, {
      display_name: message.display_name,
      verification_url: message.verification_url,
      expires_at: message.expires_at,
    });
  }

  async sendPasswordReset(message: PasswordResetMessage): Promise<void> {
    await this.#send("agent-pulse-password-reset", message.email, {
      display_name: message.display_name,
      reset_url: message.reset_url,
      expires_at: message.expires_at,
    });
  }

  async #send(
    template: string,
    email: string,
    variables: Record<string, string>,
  ): Promise<void> {
    const response = await fetch(this.#endpoint, {
      method: "POST",
      headers: {
        authorization: this.#authorization,
        "content-type": "application/json",
      },
      body: JSON.stringify({
        template,
        to: email,
        variables,
      }),
      signal: AbortSignal.timeout(10_000),
    });
    if (!response.ok) {
      throw new Error("email delivery service rejected the message");
    }
  }
}

export function verificationUrl(baseUrl: string, token: string): string {
  return secretFragmentUrl(baseUrl, "/verify-email", token);
}

export function passwordResetUrl(baseUrl: string, token: string): string {
  return secretFragmentUrl(baseUrl, "/reset-password", token);
}

function secretFragmentUrl(
  baseUrl: string,
  pathname: string,
  token: string,
): string {
  const url = new URL(pathname, baseUrl);
  if (url.protocol !== "https:" && url.hostname !== "localhost") {
    throw new Error("public account URL must use HTTPS");
  }
  url.hash = new URLSearchParams({ token }).toString();
  return url.toString();
}
