package icu.li2co3.liplayer.fabric;

/**
 * 最小 native 接口占位：按你的 JNI/JNA 方案替换方法体。
 * 当前仅定义客户端 + 本地曲库所需能力边界。
 */
public interface NativeBridge {
    boolean load();
    long create(String deviceName, String musicDir, String indexPath);
    void destroy(long handle);
    int scanLocalLibrary(long handle, String musicDir);
    int trackCount(long handle);
    String listTracksJson(long handle);
    int playTrackAt(long handle, int index);
    int pause(long handle);
    int resume(long handle);
    int stop(long handle);
    long elapsedMillis(long handle);
    int state(long handle);
}
