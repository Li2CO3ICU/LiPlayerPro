package icu.li2co3.liplayer.fabric;

/**
 * Fabric 客户端生命周期对接骨架（客户端 + 本地曲库）。
 */
public final class ClientLocalLibraryService {
    private final NativeBridge nativeBridge;
    private long handle;

    public ClientLocalLibraryService(NativeBridge nativeBridge) {
        this.nativeBridge = nativeBridge;
    }

    public void onClientStart(String deviceName, String musicDir, String indexPath) {
        if (!nativeBridge.load()) return;
        handle = nativeBridge.create(deviceName, musicDir, indexPath);
        if (handle != 0L) {
            nativeBridge.scanLocalLibrary(handle, musicDir);
        }
    }

    public void onClientStop() {
        if (handle != 0L) {
            nativeBridge.destroy(handle);
            handle = 0L;
        }
    }

    public void playByIndex(int index) {
        if (handle != 0L) nativeBridge.playTrackAt(handle, index);
    }

    public void pause() {
        if (handle != 0L) nativeBridge.pause(handle);
    }

    public void resume() {
        if (handle != 0L) nativeBridge.resume(handle);
    }

    public void stop() {
        if (handle != 0L) nativeBridge.stop(handle);
    }

    public String tracksJson() {
        return handle == 0L ? "[]" : nativeBridge.listTracksJson(handle);
    }

    public long elapsedMillis() {
        return handle == 0L ? 0L : nativeBridge.elapsedMillis(handle);
    }
}
