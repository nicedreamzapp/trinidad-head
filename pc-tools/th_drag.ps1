# A real OLE file drag: a small form at (SX,SY) starts DoDragDrop with a FileDrop, while the
# mouse is driven to (TX,TY) and released. The cursor goes back where it was afterwards.
param([string]$File, [int]$SX, [int]$SY, [int]$TX, [int]$TY)
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type -ReferencedAssemblies System.Windows.Forms, System.Drawing @"
using System; using System.Drawing; using System.Threading; using System.Windows.Forms;
using System.Runtime.InteropServices;
public class Dragger {
  [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] static extern bool GetCursorPos(out Point p);
  [DllImport("user32.dll")] static extern void mouse_event(uint f, int dx, int dy, uint d, UIntPtr e);
  public static string Run(string file, int sx, int sy, int tx, int ty) {
    Point saved; GetCursorPos(out saved);
    string result = "no-drag";
    var f = new Form();
    f.StartPosition = FormStartPosition.Manual; f.FormBorderStyle = FormBorderStyle.None;
    f.Location = new Point(sx - 40, sy - 40); f.Size = new Size(80, 80); f.TopMost = true;
    f.BackColor = Color.Magenta; f.ShowInTaskbar = false;
    f.MouseDown += (o, e) => {
      var d = new DataObject(DataFormats.FileDrop, new string[] { file });
      result = f.DoDragDrop(d, DragDropEffects.Copy | DragDropEffects.Link | DragDropEffects.Move).ToString();
      f.BeginInvoke((Action)(() => f.Close()));
    };
    f.Shown += (o, e) => {
      var t = new Thread(() => {
        Thread.Sleep(500); SetCursorPos(sx, sy); Thread.Sleep(200);
        mouse_event(2, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(300);
        for (int i = 1; i <= 25; i++) {
          SetCursorPos(sx + (tx - sx) * i / 25, sy + (ty - sy) * i / 25);
          mouse_event(1, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(30);
        }
        for (int i = 0; i < 5; i++) { SetCursorPos(tx + i, ty); mouse_event(1, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(60); }
        mouse_event(4, 0, 0, 0, UIntPtr.Zero); Thread.Sleep(1500);
        try { f.BeginInvoke((Action)(() => f.Close())); } catch {}
      });
      t.IsBackground = true; t.Start();
    };
    Application.Run(f);
    SetCursorPos(saved.X, saved.Y);
    return result;
  }
}
"@
"drag-effect=" + [Dragger]::Run($File, $SX, $SY, $TX, $TY)
