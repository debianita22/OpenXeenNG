// SPDX-License-Identifier: GPL-3.0-or-later
//
// Differential-test oracle for OpenXeen.
//
// Compiles code extracted verbatim (see build.sh) from ScummVM's Xeen engine
// and exposes it through a tiny command-line protocol, so the Rust ports can
// be compared with the reference byte for byte:
//
//   xeen_oracle rng <seed> <count>                  -> one number per line
//   xeen_oracle hash < names                        -> "XXXX" per input line
//   xeen_oracle index <file.cc>                     -> "id offset size" lines
//   xeen_oracle draw <sprite> <palette> < cases     -> 64000 bytes per case
//
// A draw case line: frame x y left top right bottom flags scale seed clippedBottom
// Each case starts from a 320x200 surface filled with (i * 7 + 3) & 0xFF.
//
// The shims below reproduce just enough of ScummVM's common classes with the
// same semantics (Common::Rect clipping, MemoryReadStream reads past the end).

#include <algorithm>
#include <cassert>
#include <climits>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iostream>
#include <iterator>
#include <sstream>
#include <string>
#include <vector>

typedef uint8_t byte;
typedef uint8_t uint8;
typedef int8_t int8;
typedef uint16_t uint16;
typedef int16_t int16;
typedef uint32_t uint32;
typedef int32_t int32;
typedef int64_t int64;
typedef unsigned int uint;

static inline uint16 READ_LE_UINT16(const void *p) {
	const byte *b = (const byte *)p;
	return (uint16)(b[0] | (b[1] << 8));
}
static inline uint32 READ_LE_UINT32(const void *p) {
	const byte *b = (const byte *)p;
	return (uint32)b[0] | ((uint32)b[1] << 8) | ((uint32)b[2] << 16) | ((uint32)b[3] << 24);
}

template<class T> static inline T MIN(T a, T b) { return a < b ? a : b; }
template<class T> static inline T MAX(T a, T b) { return a > b ? a : b; }

static void error(const char *fmt, ...) {
	fprintf(stderr, "error: %s\n", fmt);
	exit(2);
}

namespace Common {

template<class Out, class T> Out fill(Out dst, Out dstEnd, const T &value) {
	while (dst != dstEnd)
		*dst++ = value;
	return dst;
}

struct Point {
	int16 x, y;
	Point() : x(0), y(0) {}
	Point(int x_, int y_) : x((int16)x_), y((int16)y_) {}
};

struct Rect {
	int16 top, left, bottom, right;
	Rect() : top(0), left(0), bottom(0), right(0) {}
	Rect(int x1, int y1, int x2, int y2) : top((int16)y1), left((int16)x1), bottom((int16)y2), right((int16)x2) {}
	bool isValidRect() const { return left <= right && top <= bottom; }
	bool isEmpty() const { return left >= right || top >= bottom; }
	void clip(const Rect &r) {
		if (top < r.top) top = r.top;
		else if (top > r.bottom) top = r.bottom;
		if (left < r.left) left = r.left;
		else if (left > r.right) left = r.right;
		if (bottom > r.bottom) bottom = r.bottom;
		else if (bottom < r.top) bottom = r.top;
		if (right > r.right) right = r.right;
		else if (right < r.left) right = r.left;
	}
	void clip(int16 maxw, int16 maxh) { clip(Rect(0, 0, maxw, maxh)); }
};

class MemoryReadStream {
	const byte *_ptrOrig;
	const byte *_ptr;
	uint32 _size, _pos;
	bool _eos;
public:
	MemoryReadStream(const byte *data, uint32 size) : _ptrOrig(data), _ptr(data), _size(size), _pos(0), _eos(false) {}
	uint32 read(void *dataPtr, uint32 dataSize) {
		if (dataSize > _size - _pos) {
			dataSize = _size - _pos;
			_eos = true;
		}
		memcpy(dataPtr, _ptr, dataSize);
		_ptr += dataSize;
		_pos += dataSize;
		return dataSize;
	}
	byte readByte() { byte b = 0; read(&b, 1); return b; }
	uint16 readUint16LE() { byte b[2] = {0, 0}; read(b, 2); return (uint16)(b[0] | (b[1] << 8)); }
	bool seek(int64 offs, int whence = SEEK_SET) {
		assert(_pos <= _size);
		switch (whence) {
		case SEEK_END: offs = _size + offs; /* fall through */
		case SEEK_SET:
		default: _ptr = _ptrOrig + offs; _pos = (uint32)offs; break;
		case SEEK_CUR: _ptr += offs; _pos += (uint32)offs; break;
		}
		if (!(_pos <= _size)) { _pos = _size; _ptr = _ptrOrig + _pos; }
		_eos = false;
		return true;
	}
	int64 pos() const { return _pos; }
	int64 size() const { return _size; }
	bool skip(uint32 offset) { return seek(offset, SEEK_CUR); }
};
typedef MemoryReadStream SeekableReadStream;

class RandomSource {
	uint32 _randSeed;
	void scrambleSeed();
public:
	RandomSource() { setSeed(1); }
	void setSeed(uint32 seed);
	uint getRandomNumber(uint max);
	uint getRandomBit();
	uint getRandomNumberRng(uint min, uint max);
	int getRandomNumberRngSigned(int min, int max);
};

#include "random.inc"

class String {
	std::string _s;
public:
	String(const std::string &s) : _s(s) {}
	bool hasSuffix(const char *x) const {
		size_t n = strlen(x);
		return _s.size() >= n && _s.compare(_s.size() - n, n, x) == 0;
	}
	void toUppercase() { for (auto &c : _s) c = (char)toupper((unsigned char)c); }
	size_t size() const { return _s.size(); }
	const char *c_str() const { return _s.c_str(); }
};

class Path {
	std::string _p;
public:
	Path(const std::string &p) : _p(p) {}
	String baseName() const {
		size_t k = _p.find_last_of('/');
		return String(k == std::string::npos ? _p : _p.substr(k + 1));
	}
	bool empty() const { return _p.empty(); }
};

} // namespace Common

// ---------------------------------------------------------------------------
// Engine / system shims used by the sprite drawers.

class Engine {
public:
	virtual ~Engine() {}
};
Engine *g_engine = nullptr;

namespace MM {
class MMEngine : public ::Engine {
public:
	Common::RandomSource _randomSource;
	uint getRandomNumber(int max) { return _randomSource.getRandomNumber(max); }
};
} // namespace MM

struct PaletteManager {
	byte pal[768];
	void grabPalette(byte *colors, uint start, uint num) { memcpy(colors, pal + start * 3, num * 3); }
};
struct OSystem {
	PaletteManager pm;
	PaletteManager *getPaletteManager() { return &pm; }
};
OSystem *g_system = nullptr;

namespace Graphics {
struct ManagedSurface {
	int16 w = 0, h = 0;
	bool empty() const { return true; }
};
} // namespace Graphics

// Surface with generous margins: the reference code may form pointers (and
// write, when enlarging) just outside the visible area.
static const int MARGIN_ROWS = 64;

class XSurface {
	std::vector<byte> _buf;
public:
	int16 w = 0, h = 0;
	int32 pitch = 0;
	void create(int16 w_, int16 h_) {
		w = w_; h = h_; pitch = w_;
		_buf.assign((size_t)(h_ + 2 * MARGIN_ROWS) * w_, 0);
	}
	byte *getBasePtr(int x, int y) { return _buf.data() + (size_t)MARGIN_ROWS * pitch + (ptrdiff_t)y * pitch + x; }
	void addDirtyRect(const Common::Rect &) {}
	void blitFrom(const Graphics::ManagedSurface &, const Common::Point &) {}
};

namespace MM {
namespace Shared {
namespace Xeen {

#include "sprites_enums.inc"

class SpriteResource {
public:
	struct IndexEntry {
		uint16 _offset1, _offset2;
		Graphics::ManagedSurface _override;
	};
	std::vector<IndexEntry> _index;
	size_t _filesize = 0;
	byte *_data = nullptr;
	static int _clippedBottom;

	void draw(XSurface &dest, int frame, const Common::Point &destPos, uint flags = 0, int scale = 0) const;
	void draw(XSurface &dest, int frame, const Common::Point &destPos,
	          const Common::Rect &bounds, uint flags = 0, int scale = 0) const;
	static void setClippedBottom(int y) { _clippedBottom = y; }
};

#include "sprites_defs.inc"

int SpriteResource::_clippedBottom;

#include "sprites_draw.inc"

struct CCEntry {
	uint16 _id = 0;
	int _offset = 0;
	uint16 _size = 0;
	int _writeOffset = 0;
};

class BaseCCArchive {
public:
	std::vector<CCEntry> _index;
	static uint16 convertNameToId(const Common::Path &resourceName);
	void loadIndex(Common::SeekableReadStream &stream);
};

#include "cc_hash.inc"
#include "cc_index.inc"

} // namespace Xeen
} // namespace Shared
} // namespace MM

// ---------------------------------------------------------------------------

static std::vector<byte> readFile(const char *path) {
	std::ifstream f(path, std::ios::binary);
	if (!f) {
		fprintf(stderr, "cannot open %s\n", path);
		exit(2);
	}
	return std::vector<byte>((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
}

int main(int argc, char **argv) {
	using namespace MM::Shared::Xeen;
	if (argc < 2) {
		fprintf(stderr, "usage: xeen_oracle rng|hash|index|draw ...\n");
		return 2;
	}
	std::string cmd = argv[1];

	if (cmd == "rng" && argc == 4) {
		Common::RandomSource r;
		r.setSeed((uint32)strtoul(argv[2], nullptr, 10));
		long n = strtol(argv[3], nullptr, 10);
		for (long i = 0; i < n; ++i)
			printf("%u\n", r.getRandomNumber(0xffff));
		return 0;
	}

	if (cmd == "hash") {
		std::string line;
		while (std::getline(std::cin, line))
			printf("%04X\n", BaseCCArchive::convertNameToId(Common::Path(line)));
		return 0;
	}

	if (cmd == "index" && argc == 3) {
		std::vector<byte> data = readFile(argv[2]);
		Common::MemoryReadStream s(data.data(), (uint32)data.size());
		BaseCCArchive a;
		a.loadIndex(s);
		for (const CCEntry &e : a._index)
			printf("%u %d %u\n", e._id, e._offset, e._size);
		return 0;
	}

	if (cmd == "draw" && argc == 4) {
		std::vector<byte> spr = readFile(argv[2]);
		std::vector<byte> pal = readFile(argv[3]);
		if (pal.size() < 768) {
			fprintf(stderr, "palette too short\n");
			return 2;
		}
		OSystem sys;
		memcpy(sys.pm.pal, pal.data(), 768);
		g_system = &sys;
		MM::MMEngine engine;
		g_engine = &engine;

		SpriteResource res;
		res._data = spr.data();
		res._filesize = spr.size();
		Common::MemoryReadStream hdr(spr.data(), (uint32)spr.size());
		int count = hdr.readUint16LE();
		res._index.resize(count);
		for (int i = 0; i < count; ++i) {
			res._index[i]._offset1 = hdr.readUint16LE();
			res._index[i]._offset2 = hdr.readUint16LE();
		}

		std::string line;
		while (std::getline(std::cin, line)) {
			if (line.empty())
				continue;
			std::istringstream in(line);
			long frame, x, y, l, t, r, b, flags, scale, seed, clipBottom;
			in >> frame >> x >> y >> l >> t >> r >> b >> flags >> scale >> seed >> clipBottom;
			engine._randomSource.setSeed((uint32)seed);
			SpriteResource::setClippedBottom((int)clipBottom);
			XSurface dest;
			dest.create(320, 200);
			for (int yy = 0; yy < 200; ++yy)
				for (int xx = 0; xx < 320; ++xx)
					*dest.getBasePtr(xx, yy) = (byte)(((yy * 320 + xx) * 7 + 3) & 0xff);
			res.draw(dest, (int)frame, Common::Point((int)x, (int)y), Common::Rect((int)l, (int)t, (int)r, (int)b),
			         (uint)flags, (int)scale);
			for (int yy = 0; yy < 200; ++yy)
				fwrite(dest.getBasePtr(0, yy), 1, 320, stdout);
		}
		return 0;
	}

	fprintf(stderr, "bad arguments\n");
	return 2;
}
