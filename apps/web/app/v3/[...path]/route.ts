import { proxyGatewayRequest } from "../../../src/server/gateway-proxy";
export const dynamic = "force-dynamic";
export const revalidate = 0;
async function proxy(
  request: Request,
  context: { params: Promise<{ path: string[] }> },
): Promise<Response> {
  const { path } = await context.params;
  if (path[0] !== "remote" || path.includes("ws"))
    return Response.json({ error: "not_found" }, { status: 404 });
  return proxyGatewayRequest(request, path, { version: "v3" });
}
export const GET = proxy;
export const POST = proxy;
