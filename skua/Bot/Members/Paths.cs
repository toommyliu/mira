using Skua.Core.Interfaces;

namespace Mira.Skua.Bot.Members;

public sealed class Paths(IFlashUtil flash)
{
    public T Get<T>(string path, T fallback = default!) => flash.GetGameObject(path, fallback)!;

    public T Static<T>(string path, T fallback = default!) => flash.GetGameObjectStatic(path, fallback)!;

    public void Set(string path, object value) => flash.SetGameObject(path, value);

    public T Call<T>(string path, params object[] args) => flash.CallGameFunction<T>(path, args)!;

    public void Call(string path, params object[] args) => flash.CallGameFunction(path, args);
}
