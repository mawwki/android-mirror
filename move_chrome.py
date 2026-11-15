import io

p = r'C:\Users\mawwki\Desktop\android-mirror\src\main.rs'
s = io.open(p, encoding='utf-8').read()

# 1) вырезать метод (от док-комментария до fn segmented)
i_doc = s.index("    /// Рамка-хром безрисуночная")
i_seg = s.index("fn segmented<T: PartialEq + Copy>")
method = s[i_doc:i_seg]
s = s[:i_doc] + s[i_seg:]

# 2) вставить метод внутрь impl MirrorApp — между концом snackbar и '}' impl
i_snack = s.index("    fn snackbar")
i_boundary = s.index("\n    }\n}", i_snack)
ins = i_boundary + len("\n    }")
s = s[:ins] + "\n" + method.rstrip() + "\n" + s[ins:]

io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print("moved into impl ok")
