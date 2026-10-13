#include "display_attribute.h"

#include <new>
#include <oleauto.h>

extern LONG g_object_count;

void FillOpenLessInputDisplayAttribute(TF_DISPLAYATTRIBUTE *attribute) {
  if (attribute == nullptr) {
    return;
  }
  *attribute = {};
  attribute->crText.type = TF_CT_NONE;
  attribute->crBk.type = TF_CT_NONE;
  attribute->crLine.type = TF_CT_COLORREF;
  // Matches the red composition underline in the issue mock.
  attribute->crLine.cr = RGB(220, 38, 38);
  attribute->lsStyle = TF_LS_SOLID;
  attribute->fBoldLine = TRUE;
  attribute->bAttr = TF_ATTR_INPUT;
}

void ApplyOpenLessInputDisplayAttribute(TfEditCookie edit_cookie, ITfContext *context,
                                        ITfRange *range) {
  if (context == nullptr || range == nullptr) {
    return;
  }

  ITfCategoryMgr *category_mgr = nullptr;
  if (FAILED(CoCreateInstance(CLSID_TF_CategoryMgr, nullptr, CLSCTX_INPROC_SERVER,
                              IID_ITfCategoryMgr, reinterpret_cast<void **>(&category_mgr))) ||
      category_mgr == nullptr) {
    return;
  }

  TfGuidAtom atom = TF_INVALID_GUIDATOM;
  const HRESULT register_hr = category_mgr->RegisterGUID(GUID_OpenLessDisplayAttribute, &atom);
  category_mgr->Release();
  if (FAILED(register_hr) || atom == TF_INVALID_GUIDATOM) {
    return;
  }

  ITfProperty *property = nullptr;
  if (FAILED(context->GetProperty(GUID_PROP_ATTRIBUTE, &property)) || property == nullptr) {
    return;
  }

  VARIANT value;
  VariantInit(&value);
  value.vt = VT_I4;
  value.lVal = static_cast<LONG>(atom);
  property->SetValue(edit_cookie, range, &value);
  property->Release();
}

OpenLessDisplayAttributeInfo::OpenLessDisplayAttributeInfo() {
  InterlockedIncrement(&g_object_count);
}

OpenLessDisplayAttributeInfo::~OpenLessDisplayAttributeInfo() {
  InterlockedDecrement(&g_object_count);
}

STDMETHODIMP OpenLessDisplayAttributeInfo::QueryInterface(REFIID iid, void **object) {
  if (object == nullptr) {
    return E_POINTER;
  }
  *object = nullptr;
  if (iid == IID_IUnknown || iid == IID_ITfDisplayAttributeInfo) {
    *object = static_cast<ITfDisplayAttributeInfo *>(this);
    AddRef();
    return S_OK;
  }
  return E_NOINTERFACE;
}

STDMETHODIMP_(ULONG) OpenLessDisplayAttributeInfo::AddRef() {
  return static_cast<ULONG>(InterlockedIncrement(&ref_count_));
}

STDMETHODIMP_(ULONG) OpenLessDisplayAttributeInfo::Release() {
  const ULONG count = static_cast<ULONG>(InterlockedDecrement(&ref_count_));
  if (count == 0) {
    delete this;
  }
  return count;
}

STDMETHODIMP OpenLessDisplayAttributeInfo::GetGUID(GUID *guid) {
  if (guid == nullptr) {
    return E_INVALIDARG;
  }
  *guid = GUID_OpenLessDisplayAttribute;
  return S_OK;
}

STDMETHODIMP OpenLessDisplayAttributeInfo::GetDescription(BSTR *description) {
  if (description == nullptr) {
    return E_INVALIDARG;
  }
  *description = SysAllocString(L"OpenLess live transcript");
  return *description != nullptr ? S_OK : E_OUTOFMEMORY;
}

STDMETHODIMP OpenLessDisplayAttributeInfo::GetAttributeInfo(TF_DISPLAYATTRIBUTE *attribute) {
  if (attribute == nullptr) {
    return E_INVALIDARG;
  }
  FillOpenLessInputDisplayAttribute(attribute);
  return S_OK;
}

STDMETHODIMP OpenLessDisplayAttributeInfo::SetAttributeInfo(const TF_DISPLAYATTRIBUTE *attribute) {
  UNREFERENCED_PARAMETER(attribute);
  // Appearance is fixed to the issue mock; hosts may not customize it.
  return E_NOTIMPL;
}

STDMETHODIMP OpenLessDisplayAttributeInfo::Reset() { return S_OK; }

OpenLessEnumDisplayAttributeInfo::OpenLessEnumDisplayAttributeInfo() {
  InterlockedIncrement(&g_object_count);
}

OpenLessEnumDisplayAttributeInfo::~OpenLessEnumDisplayAttributeInfo() {
  InterlockedDecrement(&g_object_count);
}

STDMETHODIMP OpenLessEnumDisplayAttributeInfo::QueryInterface(REFIID iid, void **object) {
  if (object == nullptr) {
    return E_POINTER;
  }
  *object = nullptr;
  if (iid == IID_IUnknown || iid == IID_IEnumTfDisplayAttributeInfo) {
    *object = static_cast<IEnumTfDisplayAttributeInfo *>(this);
    AddRef();
    return S_OK;
  }
  return E_NOINTERFACE;
}

STDMETHODIMP_(ULONG) OpenLessEnumDisplayAttributeInfo::AddRef() {
  return static_cast<ULONG>(InterlockedIncrement(&ref_count_));
}

STDMETHODIMP_(ULONG) OpenLessEnumDisplayAttributeInfo::Release() {
  const ULONG count = static_cast<ULONG>(InterlockedDecrement(&ref_count_));
  if (count == 0) {
    delete this;
  }
  return count;
}

STDMETHODIMP OpenLessEnumDisplayAttributeInfo::Clone(IEnumTfDisplayAttributeInfo **enum_info) {
  if (enum_info == nullptr) {
    return E_INVALIDARG;
  }
  auto *clone = new (std::nothrow) OpenLessEnumDisplayAttributeInfo();
  if (clone == nullptr) {
    return E_OUTOFMEMORY;
  }
  clone->index_ = index_;
  *enum_info = clone;
  return S_OK;
}

STDMETHODIMP OpenLessEnumDisplayAttributeInfo::Next(ULONG count, ITfDisplayAttributeInfo **info,
                                                    ULONG *fetched) {
  if (info == nullptr) {
    return E_INVALIDARG;
  }
  ULONG produced = 0;
  while (produced < count && index_ == 0) {
    auto *attribute = new (std::nothrow) OpenLessDisplayAttributeInfo();
    if (attribute == nullptr) {
      break;
    }
    info[produced] = attribute;
    ++produced;
    ++index_;
  }
  if (fetched != nullptr) {
    *fetched = produced;
  }
  return produced == count ? S_OK : S_FALSE;
}

STDMETHODIMP OpenLessEnumDisplayAttributeInfo::Reset() {
  index_ = 0;
  return S_OK;
}

STDMETHODIMP OpenLessEnumDisplayAttributeInfo::Skip(ULONG count) {
  if (count == 0) {
    return S_OK;
  }
  if (index_ == 0 && count >= 1) {
    index_ = 1;
    return count == 1 ? S_OK : S_FALSE;
  }
  return S_FALSE;
}
