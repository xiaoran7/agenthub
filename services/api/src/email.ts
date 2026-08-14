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

export interface TeamInvitationMessage {
  email: string;
  display_name: string;
  inviter_name: string;
  invitation_url: string;
  role: "admin" | "member";
  expires_at: string;
}

export interface EmailVerificationSender {
  sendVerification(message: EmailVerificationMessage): Promise<void>;
  sendPasswordReset?(message: PasswordResetMessage): Promise<void>;
  sendTeamInvitation?(message: TeamInvitationMessage): Promise<void>;
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

  async sendTeamInvitation(message: TeamInvitationMessage): Promise<void> {
    await this.#send("agent-pulse-team-invitation", message.email, {
      display_name: message.display_name,
      inviter_name: message.inviter_name,
      invitation_url: message.invitation_url,
      role: message.role,
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

export class ResendEmailSender implements EmailVerificationSender {
  readonly #endpoint = "https://api.resend.com/emails";
  readonly #authorization: string;
  readonly #from: string;
  readonly #fetch: typeof fetch;

  constructor(
    apiKey: string,
    from: string,
    fetcher: typeof fetch = globalThis.fetch,
  ) {
    if (
      !apiKey.startsWith("re_") ||
      apiKey.length < 16 ||
      apiKey.length > 512 ||
      /[\u0000-\u001f\u007f]/u.test(apiKey)
    ) {
      throw new Error("Resend API key is invalid");
    }
    if (
      from.length < 3 ||
      from.length > 320 ||
      /[\u0000-\u001f\u007f]/u.test(from)
    ) {
      throw new Error("email sender address is invalid");
    }
    this.#authorization = `Bearer ${apiKey}`;
    this.#from = from;
    this.#fetch = fetcher;
  }

  async sendVerification(message: EmailVerificationMessage): Promise<void> {
    await this.#send(
      message.email,
      "验证你的 AgentPulse 邮箱",
      `你好 ${message.display_name}，请打开以下链接验证邮箱：${message.verification_url}\n\n链接有效期至：${message.expires_at}`,
      `<p>你好 ${escapeHtml(message.display_name)}，请验证你的 AgentPulse 邮箱。</p><p><a href="${escapeHtml(message.verification_url)}">验证邮箱</a></p><p>链接有效期至：${escapeHtml(message.expires_at)}</p>`,
    );
  }

  async sendPasswordReset(message: PasswordResetMessage): Promise<void> {
    await this.#send(
      message.email,
      "重置你的 AgentPulse 密码",
      `你好 ${message.display_name}，请打开以下链接设置新密码：${message.reset_url}\n\n链接有效期至：${message.expires_at}`,
      `<p>你好 ${escapeHtml(message.display_name)}，请设置新的 AgentPulse 密码。</p><p><a href="${escapeHtml(message.reset_url)}">设置新密码</a></p><p>链接有效期至：${escapeHtml(message.expires_at)}</p>`,
    );
  }

  async sendTeamInvitation(message: TeamInvitationMessage): Promise<void> {
    await this.#send(
      message.email,
      `${message.inviter_name} 邀请你加入 AgentPulse Team`,
      `${message.inviter_name} 邀请你加入 AgentPulse Team 工作区。\n\n接受邀请：${message.invitation_url}\n\n链接有效期至：${message.expires_at}`,
      `<p>${escapeHtml(message.inviter_name)} 邀请你加入 AgentPulse Team 工作区。</p><p><a href="${escapeHtml(message.invitation_url)}">接受邀请</a></p><p>链接有效期至：${escapeHtml(message.expires_at)}</p>`,
    );
  }

  async #send(
    to: string,
    subject: string,
    text: string,
    html: string,
  ): Promise<void> {
    const response = await this.#fetch(this.#endpoint, {
      method: "POST",
      headers: {
        authorization: this.#authorization,
        "content-type": "application/json",
      },
      body: JSON.stringify({ from: this.#from, to, subject, text, html }),
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

export function teamInvitationUrl(baseUrl: string, token: string): string {
  const url = new URL("/", baseUrl);
  if (url.protocol !== "https:" && url.hostname !== "localhost") {
    throw new Error("public account URL must use HTTPS");
  }
  url.hash = new URLSearchParams({ team_invitation: token }).toString();
  return url.toString();
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

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/gu, (character) => {
    switch (character) {
      case "&":
        return "&amp;";
      case "<":
        return "&lt;";
      case ">":
        return "&gt;";
      case '"':
        return "&quot;";
      default:
        return "&#39;";
    }
  });
}
