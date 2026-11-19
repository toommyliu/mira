namespace Mira.Skua.Bot.Members;

public sealed class Send(Game game, Paths paths, CancellationToken stopping) : global::Skua.Core.Interfaces.IScriptSend
{
    public void Packet(string packet, string type = "String")
    {
        if (type == "String")
            game.Request("packets", "sendToServer", new { packet });
        else
            paths.Call($"sfc.send{type}", packet);
    }

    public Task PacketSpam(string packet, string type, int delay, CancellationToken token) =>
        Spam(() => Packet(packet, type), delay, token);

    public void ClientPacket(string packet, string type = "str") =>
        game.Loader("flash.sendClientPacket", [packet, type]);

    public Task ClientPacketSpam(string packet, string type, int delay, CancellationToken token) =>
        Spam(() => ClientPacket(packet, type), delay, token);

    public void Whisper(string name, string message) => Packet($"%xt%zm%whisper%1%{message}%{name}%");

    public void ClientServer(string message, string sentBy = "Skua") => Chat(message, sentBy, "server");

    public void ClientModerator(string message, string sentBy = "Skua") => Chat(message, sentBy, "moderator");

    public void ClientWarning(string message, string sentBy = "Skua") => Chat(message, sentBy, "warning");

    public void ClientEvent(string message, string sentBy = "Skua") => Chat(message, sentBy, "event");

    public void ClientGuild(string message, string sentBy = "Skua") => Chat(message, sentBy, "guild");

    public void ClientWhisper(string message, string sentBy = "Skua") => Chat(message, sentBy, "whisper");

    public void ClientZone(string message, string sentBy = "Skua") => Chat(message, sentBy, "zone");

    private void Chat(string message, string sentBy, string kind) =>
        ClientPacket($"%xt%chatm%0%{kind}~{message}%{sentBy}%", "str");

    private Task Spam(Action send, int delay, CancellationToken token) => Task.Run(async () =>
    {
        using var both = CancellationTokenSource.CreateLinkedTokenSource(token, stopping);
        while (!both.IsCancellationRequested)
        {
            send();
            await Task.Delay(delay, both.Token);
        }
    }, token);
}
